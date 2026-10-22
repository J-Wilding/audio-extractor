//! Playlist File generation (extended `.m3u`) and Plex import (see CONTEXT.md).
//! The Plex token is sent only as a header and never logged.

use std::fs;
use std::time::Duration;

use log::{error, info, warn};
use serde::{Deserialize, Serialize};

use crate::config::PlexConfig;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TrackMetadata {
    pub filepath: String,
    pub title: String,
    pub artist: String,
    /// Seconds.
    pub duration: u64,
}

#[derive(Clone, Debug)]
pub struct PlaylistBatch {
    pub playlist_title: String,
    pub tracks: Vec<TrackMetadata>,
}

/// Sanitizes a string for use as a filename on Linux and SMB shares.
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

/// Rewrites a path under `local_root` to the same path as the Plex server sees it.
/// Paths outside `local_root` are returned unchanged.
pub fn local_to_plex_path(local_path: &str, local_root: &str, plex_root: &str) -> String {
    let path = local_path.trim_end_matches('/');
    let local_root = local_root.trim_end_matches('/');
    let plex_root = plex_root.trim_end_matches('/');
    match path.strip_prefix(local_root) {
        // Only match whole path components (`/music2` is not under `/music`).
        Some(rest) if rest.is_empty() || rest.starts_with('/') => format!("{}{}", plex_root, rest),
        _ => path.to_string(),
    }
}

/// Builds extended M3U content with Plex-side absolute paths.
pub fn generate_extended_m3u(tracks: &[TrackMetadata], local_root: &str, plex_root: &str) -> String {
    let mut out = String::from("#EXTM3U\n");
    for t in tracks {
        out.push_str(&format!("#EXTINF:{},{} - {}\n", t.duration, t.artist, t.title));
        out.push_str(&local_to_plex_path(&t.filepath, local_root, plex_root));
        out.push('\n');
    }
    out
}

/// Writes the Playlist File into the playlists directory.
/// Returns `(local_path, plex_path)` of the written `.m3u`.
pub fn write_playlist_m3u(batch: &PlaylistBatch, cfg: &PlexConfig) -> Result<(String, String), String> {
    fs::create_dir_all(&cfg.playlists_dir)
        .map_err(|e| format!("Failed to create playlists dir {:?}: {}", cfg.playlists_dir, e))?;

    let local_path = cfg
        .playlists_dir
        .join(format!("{}.m3u", sanitize_filename(&batch.playlist_title)));
    let content = generate_extended_m3u(&batch.tracks, &cfg.local_music_root, &cfg.plex_music_root);
    fs::write(&local_path, content)
        .map_err(|e| format!("Failed to write M3U file at {:?}: {}", local_path, e))?;
    info!("Wrote playlist file to {:?}", local_path);

    let local = local_path.to_string_lossy().into_owned();
    let plex = local_to_plex_path(&local, &cfg.local_music_root, &cfg.plex_music_root);
    Ok((local, plex))
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .unwrap_or_default()
}

/// Triggers a Plex library section refresh.
pub async fn trigger_plex_refresh(cfg: &PlexConfig) -> Result<(), String> {
    let token = cfg.token.as_deref().ok_or("Plex token is not configured.")?;
    let url = format!("{}/library/sections/{}/refresh", cfg.url, cfg.section_id);
    info!("Triggering Plex library scan at {}", url);

    let res = client()
        .get(&url)
        .header("X-Plex-Token", token)
        .send()
        .await
        .map_err(|e| format!("Failed to send refresh request to Plex at {}: {}", cfg.url, e))?;
    if res.status().is_success() {
        Ok(())
    } else {
        warn!("Plex library refresh returned HTTP {}", res.status());
        Err(format!("Plex refresh failed with status HTTP {}", res.status()))
    }
}

/// Whether the configured Plex section is currently scanning.
pub async fn is_section_refreshing(cfg: &PlexConfig) -> bool {
    let Some(token) = cfg.token.as_deref() else { return false };
    let url = format!("{}/library/sections", cfg.url);
    let Ok(res) = client().get(&url).header("X-Plex-Token", token).send().await else {
        return false;
    };
    let Ok(body) = res.text().await else { return false };
    section_is_refreshing(&body, &cfg.section_id)
}

/// Parses `/library/sections` XML for `<Directory key="{id}" ... refreshing="1">`.
fn section_is_refreshing(xml: &str, section_id: &str) -> bool {
    let target_key = format!("key=\"{}\"", section_id);
    xml.split('<')
        .find(|el| el.starts_with("Directory") && el.contains(&target_key))
        .is_some_and(|el| el.contains("refreshing=\"1\""))
}

/// Waits (up to `timeout_secs`) for an in-flight scan of the section to finish.
pub async fn wait_for_plex_refresh(cfg: &PlexConfig, timeout_secs: u64) {
    let start = std::time::Instant::now();
    // Brief pause so Plex has a chance to mark the section as refreshing.
    tokio::time::sleep(Duration::from_millis(500)).await;
    while start.elapsed().as_secs() < timeout_secs {
        if !is_section_refreshing(cfg).await {
            info!("Plex section {} scan completed.", cfg.section_id);
            return;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    warn!("Timed out waiting for Plex scan to complete after {}s", timeout_secs);
}

/// Imports an `.m3u` (Plex-side path) via `POST /playlists/upload`, with retries.
pub async fn upload_playlist_to_plex(plex_m3u_path: &str, cfg: &PlexConfig) -> Result<(), String> {
    let token = cfg.token.as_deref().ok_or("Plex token is not configured.")?;
    let mut url = url::Url::parse(&format!("{}/playlists/upload", cfg.url))
        .map_err(|e| format!("Invalid Plex base URL ({}): {}", cfg.url, e))?;
    url.query_pairs_mut()
        .append_pair("sectionID", &cfg.section_id)
        .append_pair("path", plex_m3u_path);

    info!("Importing playlist into Plex from server path: {}", plex_m3u_path);
    let client = client();
    let mut last_err = String::new();
    // Plex returns 500 while still indexing the new files; retry with a short backoff.
    for attempt in 1..=4 {
        match client.post(url.as_str()).header("X-Plex-Token", token).send().await {
            Ok(resp) if resp.status().is_success() => {
                info!("Playlist imported into Plex on attempt {}.", attempt);
                return Ok(());
            }
            Ok(resp) => {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                last_err = format!("HTTP {} - {}", status, body);
                warn!("Plex playlist upload attempt {} returned {}.", attempt, status);
            }
            Err(e) => {
                last_err = e.to_string();
                warn!("Plex playlist upload attempt {} failed: {}.", attempt, e);
            }
        }
        tokio::time::sleep(Duration::from_millis(1500)).await;
    }
    error!("Plex playlist upload failed after 4 attempts: {}", last_err);
    Err(format!("Plex playlist upload failed: {}", last_err))
}

/// Writes the Playlist File, scans Plex and imports it. Plex failures are reported
/// in the `Ok` message so they never fail the download itself.
pub async fn handle_playlist_import(batch: PlaylistBatch, cfg: &PlexConfig) -> Result<String, String> {
    if batch.tracks.is_empty() {
        return Ok("Playlist had no tracks to import.".to_string());
    }
    let (local_path, plex_path) = write_playlist_m3u(&batch, cfg)?;

    if cfg.token.is_none() {
        return Ok(format!("Generated {}. (PLEX_TOKEN not set; skipped Plex import)", local_path));
    }
    if let Err(e) = trigger_plex_refresh(cfg).await {
        warn!("{}", e);
    }
    wait_for_plex_refresh(cfg, 15).await;

    match upload_playlist_to_plex(&plex_path, cfg).await {
        Ok(()) => Ok(format!("Imported playlist '{}' into Plex.", batch.playlist_title)),
        Err(e) => Ok(format!("Saved {}, but Plex import failed: {}", local_path, e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOCAL: &str = "/music";
    const PLEX: &str = "/share/CACHEDEV1_DATA/Multimedia/Media/Music";

    #[test]
    fn test_sanitize_filename() {
        assert_eq!(sanitize_filename("Rock & Roll: Hits/Favorites?"), "Rock & Roll_ Hits_Favorites_");
        assert_eq!(sanitize_filename("   "), "Untitled Playlist");
    }

    #[test]
    fn test_local_to_plex_path() {
        assert_eq!(
            local_to_plex_path("/music/Artist/Album/01 - Track.m4a", LOCAL, PLEX),
            format!("{}/Artist/Album/01 - Track.m4a", PLEX)
        );
        assert_eq!(local_to_plex_path("/music/", LOCAL, PLEX), PLEX);
        // Not a path-component match: left alone.
        assert_eq!(local_to_plex_path("/music2/x.m4a", LOCAL, PLEX), "/music2/x.m4a");
        assert_eq!(local_to_plex_path("/other/x.m4a", LOCAL, PLEX), "/other/x.m4a");
    }

    #[test]
    fn test_generate_extended_m3u() {
        let tracks = vec![
            TrackMetadata {
                filepath: "/music/Artist A/Album 1/01 - Song.m4a".into(),
                title: "Song".into(),
                artist: "Artist A".into(),
                duration: 215,
            },
            TrackMetadata {
                filepath: "/music/Artist B/Album 2/02 - Track.m4a".into(),
                title: "Track".into(),
                artist: "Artist B".into(),
                duration: 180,
            },
        ];
        let expected = format!(
            "#EXTM3U\n#EXTINF:215,Artist A - Song\n{p}/Artist A/Album 1/01 - Song.m4a\n#EXTINF:180,Artist B - Track\n{p}/Artist B/Album 2/02 - Track.m4a\n",
            p = PLEX
        );
        assert_eq!(generate_extended_m3u(&tracks, LOCAL, PLEX), expected);
    }

    #[test]
    fn test_section_is_refreshing() {
        let xml = r#"<MediaContainer><Directory key="1" refreshing="1" title="Movies"><Location/></Directory><Directory key="3" refreshing="0" title="Music"/></MediaContainer>"#;
        assert!(section_is_refreshing(xml, "1"));
        assert!(!section_is_refreshing(xml, "3"));
        assert!(!section_is_refreshing(xml, "9"));
    }

    #[test]
    fn write_playlist_uses_plex_paths() {
        let dir = std::env::temp_dir().join(format!("aes-m3u-{}", std::process::id()));
        let cfg = PlexConfig {
            url: "http://plex".into(),
            token: None,
            section_id: "3".into(),
            local_music_root: dir.to_string_lossy().into_owned(),
            plex_music_root: PLEX.into(),
            playlists_dir: dir.join("Playlists"),
        };
        let batch = PlaylistBatch {
            playlist_title: "Mix: 1".into(),
            tracks: vec![TrackMetadata {
                filepath: dir.join("A/B/01 - C.m4a").to_string_lossy().into_owned(),
                title: "C".into(),
                artist: "A".into(),
                duration: 1,
            }],
        };
        let (local, plex) = write_playlist_m3u(&batch, &cfg).unwrap();
        assert!(local.ends_with("Playlists/Mix_ 1.m3u"));
        assert_eq!(plex, format!("{}/Playlists/Mix_ 1.m3u", PLEX));
        let content = fs::read_to_string(&local).unwrap();
        assert!(content.contains(&format!("{}/A/B/01 - C.m4a", PLEX)));
        fs::remove_dir_all(&dir).unwrap();
    }
}
