use std::fs;
use std::path::{Path, PathBuf};
use log::{error, info, warn};
use serde::{Deserialize, Serialize};

use crate::config::AppConfig;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TrackMetadata {
    pub filepath: String,
    pub title: String,
    pub artist: String,
    pub duration: u64,
}

#[derive(Clone, Debug)]
pub struct PlaylistBatch {
    pub playlist_title: String,
    pub tracks: Vec<TrackMetadata>,
}

/// Sanitizes a string for use as a filename on macOS and Linux file shares.
pub fn sanitize_filename(name: &str) -> String {
    let sanitized: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            other => other,
        })
        .collect();
    let trimmed = sanitized.trim();
    if trimmed.is_empty() {
        "Untitled Playlist".to_string()
    } else {
        trimmed.to_string()
    }
}

pub use crate::config::normalize_plex_url;

/// Translates a local Mac file path into its equivalent Plex server path using prefix replacement.
pub fn mac_to_plex_path(mac_path: &str, mac_root: &str, plex_root: &str) -> String {
    let norm_mac = mac_path.trim_end_matches('/');
    let norm_mac_root = mac_root.trim_end_matches('/');
    let norm_plex_root = plex_root.trim_end_matches('/');

    if norm_mac.starts_with(norm_mac_root) {
        let remainder = &norm_mac[norm_mac_root.len()..];
        format!("{}{}", norm_plex_root, remainder)
    } else {
        norm_mac.to_string()
    }
}

/// Generates an extended M3U file content for Plex.
pub fn generate_extended_m3u(
    tracks: &[TrackMetadata],
    mac_root: &str,
    plex_root: &str,
) -> String {
    let mut out = String::from("#EXTM3U\n");
    for t in tracks {
        let plex_path = mac_to_plex_path(&t.filepath, mac_root, plex_root);
        out.push_str(&format!("#EXTINF:{},{} - {}\n", t.duration, t.artist, t.title));
        out.push_str(&format!("{}\n", plex_path));
    }
    out
}

/// Writes the extended M3U file to the playlists directory on the NAS.
/// Returns (local_mac_m3u_path, plex_m3u_path).
pub fn write_playlist_m3u(
    batch: &PlaylistBatch,
    config: &AppConfig,
) -> Result<(PathBuf, String), String> {
    let playlists_dir = Path::new(&config.playlists_dir);
    if !playlists_dir.exists() {
        fs::create_dir_all(playlists_dir)
            .map_err(|e| format!("Failed to create playlists dir {:?}: {}", playlists_dir, e))?;
    }

    let safe_title = sanitize_filename(&batch.playlist_title);
    let m3u_filename = format!("{}.m3u", safe_title);
    let local_path = playlists_dir.join(&m3u_filename);

    let m3u_content = generate_extended_m3u(&batch.tracks, &config.mac_music_root, &config.plex_music_root);
    fs::write(&local_path, m3u_content)
        .map_err(|e| format!("Failed to write M3U file at {:?}: {}", local_path, e))?;

    info!("Wrote playlist file to {:?}", local_path);

    let plex_path = mac_to_plex_path(
        &local_path.to_string_lossy(),
        &config.mac_music_root,
        &config.plex_music_root,
    );

    Ok((local_path, plex_path))
}

/// Triggers a Plex library section refresh.
pub async fn trigger_plex_refresh(config: &AppConfig) -> Result<(), String> {
    let token = config.plex_token.as_deref().ok_or("Plex token is not configured.")?;
    let base_url = normalize_plex_url(&config.plex_url);
    let url = format!("{}/library/sections/{}/refresh", base_url, config.plex_section_id);

    info!("Triggering Plex library scan at {}", url);
    let client = reqwest::Client::new();
    let res = client
        .get(&url)
        .header("X-Plex-Token", token)
        .send()
        .await
        .map_err(|e| format!("Failed to send refresh request to Plex: {}", e))?;

    if res.status().is_success() {
        info!("Plex library refresh triggered successfully.");
        Ok(())
    } else {
        let status = res.status();
        warn!("Plex library refresh returned HTTP {}", status);
        Err(format!("Plex refresh failed with status HTTP {}", status))
    }
}

/// Checks whether the Plex library section is currently refreshing/scanning.
pub async fn is_section_refreshing(config: &AppConfig) -> bool {
    let Some(token) = config.plex_token.as_deref() else { return false };
    let base_url = normalize_plex_url(&config.plex_url);
    let url = format!("{}/library/sections", base_url);

    let client = reqwest::Client::new();
    let Ok(res) = client.get(&url).header("X-Plex-Token", token).send().await else {
        return false;
    };

    let Ok(body) = res.text().await else {
        return false;
    };

    // Parse simple XML looking for <Directory key="{section_id}" refreshing="1">
    let target_key = format!("key=\"{}\"", config.plex_section_id);
    for line in body.split("/>") {
        if line.contains(&target_key) {
            return line.contains("refreshing=\"1\"");
        }
    }
    false
}

/// Waits for an in-flight Plex library scan to complete (up to timeout_secs).
pub async fn wait_for_plex_refresh(config: &AppConfig, timeout_secs: u64) {
    let start = std::time::Instant::now();
    // Brief initial pause to let Plex mark the section as refreshing
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    while start.elapsed().as_secs() < timeout_secs {
        if !is_section_refreshing(config).await {
            info!("Plex section {} scan completed.", config.plex_section_id);
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    warn!("Timed out waiting for Plex scan to complete after {}s", timeout_secs);
}

/// Uploads/imports an M3U file into Plex with retries.
pub async fn upload_playlist_to_plex(plex_m3u_path: &str, config: &AppConfig) -> Result<(), String> {
    let token = config.plex_token.as_deref().ok_or("Plex token is not configured.")?;
    let base_url = normalize_plex_url(&config.plex_url);
    
    let mut url = match url::Url::parse(&format!("{}/playlists/upload", base_url)) {
        Ok(u) => u,
        Err(e) => return Err(format!("Invalid Plex base URL ({}): {}", base_url, e)),
    };
    
    url.query_pairs_mut()
        .append_pair("sectionID", &config.plex_section_id)
        .append_pair("path", plex_m3u_path);

    info!("Importing playlist into Plex from server path: {}", plex_m3u_path);
    let client = reqwest::Client::new();

    let mut last_err = String::new();
    // Retry up to 4 times with short backoff (helps if Plex is still indexing metadata)
    for attempt in 1..=4 {
        let res = client
            .post(url.as_str())
            .header("X-Plex-Token", token)
            .send()
            .await;

        match res {
            Ok(resp) if resp.status().is_success() => {
                info!("Playlist successfully imported/updated in Plex on attempt {}.", attempt);
                return Ok(());
            }
            Ok(resp) => {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                last_err = format!("HTTP {} - {}", status, body);
                warn!("Plex playlist upload attempt {} returned {}. Retrying in 1.5s...", attempt, status);
            }
            Err(e) => {
                last_err = e.to_string();
                warn!("Plex playlist upload attempt {} failed: {}. Retrying...", attempt, e);
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
    }

    error!("Plex playlist upload failed after 4 attempts: {}", last_err);
    Err(format!("Plex playlist upload failed: {}", last_err))
}

/// Orchestrates post-download playlist generation and Plex import.
pub async fn handle_playlist_import(
    batch: PlaylistBatch,
    config: &AppConfig,
) -> Result<String, String> {
    if batch.tracks.is_empty() {
        return Ok("Playlist had no tracks to import.".to_string());
    }

    let (local_path, plex_path) = write_playlist_m3u(&batch, config)?;

    if config.plex_token.is_none() {
        return Ok(format!(
            "Generated M3U at {:?}. (Plex token not set; skipped Plex import)",
            local_path.file_name().unwrap_or_default()
        ));
    }

    // Trigger library scan so Plex indexes the newly placed audio files
    let _ = trigger_plex_refresh(config).await;

    // Wait until Plex finishes scanning the music section (up to 15 seconds)
    wait_for_plex_refresh(config, 15).await;

    match upload_playlist_to_plex(&plex_path, config).await {
        Ok(_) => Ok(format!(
            "Imported playlist '{}' into Plex ({})",
            batch.playlist_title,
            local_path.file_name().unwrap_or_default().to_string_lossy()
        )),
        Err(e) => Ok(format!(
            "Saved M3U to {:?}, but Plex import failed: {}",
            local_path.file_name().unwrap_or_default(),
            e
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_filename() {
        assert_eq!(sanitize_filename("Rock & Roll: Hits/Favorites?"), "Rock & Roll_ Hits_Favorites_");
        assert_eq!(sanitize_filename("   "), "Untitled Playlist");
    }

    #[test]
    fn test_mac_to_plex_path() {
        let mac = "/Volumes/data/Media/Music/Artist/Album/01 - Track.m4a";
        let plex = mac_to_plex_path(
            mac,
            "/Volumes/data/Media/Music",
            "/share/CACHEDEV1_DATA/Multimedia/Media/Music",
        );
        assert_eq!(plex, "/share/CACHEDEV1_DATA/Multimedia/Media/Music/Artist/Album/01 - Track.m4a");
    }

    #[test]
    fn test_generate_extended_m3u() {
        let tracks = vec![
            TrackMetadata {
                filepath: "/Volumes/data/Media/Music/Artist A/Album 1/01 - Song.m4a".into(),
                title: "Song".into(),
                artist: "Artist A".into(),
                duration: 215,
            },
            TrackMetadata {
                filepath: "/Volumes/data/Media/Music/Artist B/Album 2/02 - Track.m4a".into(),
                title: "Track".into(),
                artist: "Artist B".into(),
                duration: 180,
            },
        ];

        let m3u = generate_extended_m3u(
            &tracks,
            "/Volumes/data/Media/Music",
            "/share/CACHEDEV1_DATA/Multimedia/Media/Music",
        );

        let expected = "#EXTM3U\n#EXTINF:215,Artist A - Song\n/share/CACHEDEV1_DATA/Multimedia/Media/Music/Artist A/Album 1/01 - Song.m4a\n#EXTINF:180,Artist B - Track\n/share/CACHEDEV1_DATA/Multimedia/Media/Music/Artist B/Album 2/02 - Track.m4a\n";
        assert_eq!(m3u, expected);
    }
}
