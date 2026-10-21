use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager};
use log::{info, error, debug, warn};

use crate::config::AppConfig;
use crate::engine::EngineLock;
use crate::plex::{self, PlaylistBatch, TrackMetadata};

/// The default NAS Root where all extracted audio is routed (see CONTEXT.md).
#[allow(dead_code)]
pub const NAS_ROOT: &str = "/Volumes/data/Media/Music/";

pub const TRACK_DELIM_PREFIX: &str = "YTDLM_TRACK:::";

/// Everything the extraction core needs, with no dependency on Tauri.
/// Entry points (the GUI command, the upcoming Browser Bridge) build one of these.
pub struct ExtractionRequest<'a> {
    pub engine_path: &'a Path,
    pub nas_root: &'a Path,
    pub url: &'a str,
    pub config: &'a AppConfig,
}

/// The relative output template under the NAS Root (Metadata Routing).
pub const OUTPUT_TEMPLATE: &str =
    "%(artist,uploader|Unknown Artist)s/%(album,playlist|Singles)s/%(playlist_index,track_number&{:02d} - |)s%(title)s.%(ext)s";

/// Reads `reader` to EOF, invoking `emit` for every non-empty line.
/// Both `\n` and `\r` are treated as line terminators so that yt-dlp's
/// in-place (`\r`) progress updates arrive as individual lines.
fn stream_lines<R: Read>(reader: R, mut emit: impl FnMut(String)) {
    let mut buf = Vec::new();
    for byte in BufReader::new(reader).bytes() {
        let Ok(c) = byte else { break };
        if c == b'\n' || c == b'\r' {
            if !buf.is_empty() {
                emit(String::from_utf8_lossy(&buf).to_string());
                buf.clear();
            }
        } else {
            buf.push(c);
        }
    }
    if !buf.is_empty() {
        emit(String::from_utf8_lossy(&buf).to_string());
    }
}

/// Parses a metadata line emitted via yt-dlp's `--print after_move:...`
/// Returns (TrackMetadata, playlist_title, playlist_id, album_name).
pub fn parse_track_delim_line(line: &str) -> Option<(TrackMetadata, String, String, String)> {
    if !line.starts_with(TRACK_DELIM_PREFIX) {
        return None;
    }
    let parts: Vec<&str> = line[TRACK_DELIM_PREFIX.len()..].split(":::").collect();
    if parts.len() < 7 {
        return None;
    }

    let filepath = parts[0].to_string();
    let title = parts[1].to_string();
    let artist = parts[2].to_string();
    let duration = parts[3].parse::<f64>().map(|d| d.round() as u64).unwrap_or(0);
    let playlist_title = parts[4].to_string();
    let playlist_id = parts[5].to_string();
    let album = parts[6].to_string();

    Some((
        TrackMetadata {
            filepath,
            title,
            artist,
            duration,
        },
        playlist_title,
        playlist_id,
        album,
    ))
}

/// Determines if a playlist should generate an M3U file for Plex.
/// Album playlists (e.g. YouTube Music OLAK5uy_ IDs) already map into Plex albums,
/// so they should not generate separate M3U playlists.
pub fn is_custom_playlist(playlist_id: &str, playlist_title: &str, album: &str) -> bool {
    if playlist_id == "NA" || playlist_id.is_empty() {
        return false;
    }
    // YouTube Music album playlists start with OLAK5uy_
    if playlist_id.starts_with("OLAK5uy_") {
        return false;
    }
    // If the playlist title is empty or NA
    if playlist_title == "NA" || playlist_title.is_empty() {
        return false;
    }
    // If album name equals playlist title, it is treated as an album
    if album != "NA" && !album.is_empty() && album.eq_ignore_ascii_case(playlist_title) {
        return false;
    }
    true
}

/// The extraction core: validates the NAS Root and Engine, runs the Engine
/// against `req.url`, and reports every stdout/stderr line through `on_progress`.
///
/// This function blocks until the Engine exits. Callers serialize jobs via [`EngineLock`].
pub fn extract<F>(req: &ExtractionRequest, on_progress: F) -> Result<String, String>
where
    F: Fn(String) + Sync,
{
    let nas_root = req.nas_root;
    debug!("Validating NAS Root at {:?}", nas_root);
    if !nas_root.exists() {
        warn!("NAS Validation Failed: Directory does not exist.");
        return Err(format!(
            "NAS is not mounted or accessible at {}. Please mount your NAS first.",
            nas_root.display()
        ));
    }

    let engine_path = req.engine_path;
    debug!("Using Engine binary at {:?}", engine_path);
    if !engine_path.exists() {
        error!("Engine binary is completely missing at runtime.");
        return Err("Engine not found. Please wait for it to download or restart the app.".to_string());
    }

    let temp_dir = std::env::temp_dir().join("audio-extractor-temp");
    let _ = std::fs::create_dir_all(&temp_dir);

    info!("Starting extraction process for URL: {}", req.url);
    debug!("Using NAS Root: {:?}", nas_root);
    debug!("Using Output Template: {}", OUTPUT_TEMPLATE);

    let current_path = std::env::var("PATH").unwrap_or_default();
    let new_path = format!("{}:/opt/homebrew/bin:/usr/local/bin", current_path);

    let print_arg = format!(
        "after_move:{}%(filepath)s:::%(title)s:::%(artist,uploader|Unknown Artist)s:::%(duration)s:::%(playlist_title)s:::%(playlist_id)s:::%(album)s",
        TRACK_DELIM_PREFIX
    );

    let mut child = Command::new(engine_path)
        .env("PATH", new_path)
        .arg("-x")
        .arg("--audio-format")
        .arg("m4a")
        .arg("--embed-metadata")
        .arg("--no-part")
        .arg("--paths")
        .arg(format!("home:{}", nas_root.display()))
        .arg("--paths")
        .arg(format!("temp:{}", temp_dir.display()))
        .arg("-o")
        .arg(OUTPUT_TEMPLATE)
        .arg("--print")
        .arg(&print_arg)
        .arg(req.url)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            error!("Failed to even spawn the engine subprocess: {}", e);
            format!("Failed to spawn engine: {}", e)
        })?;

    let stdout = child.stdout.take().ok_or("Failed to open engine stdout")?;
    let stderr = child.stderr.take().ok_or("Failed to open engine stderr")?;

    let collected_tracks = Arc::new(Mutex::new(Vec::new()));
    let playlist_meta = Arc::new(Mutex::new((String::new(), String::new(), String::new()))); // (title, id, album)

    let tracks_clone = collected_tracks.clone();
    let meta_clone = playlist_meta.clone();

    let on_progress_ref = &on_progress;
    std::thread::scope(|s| {
        s.spawn(move || {
            stream_lines(stdout, |line| {
                if let Some((track, p_title, p_id, album)) = parse_track_delim_line(&line) {
                    tracks_clone.lock().unwrap().push(track);
                    let mut meta = meta_clone.lock().unwrap();
                    if meta.0.is_empty() && p_title != "NA" {
                        meta.0 = p_title;
                        meta.1 = p_id;
                        meta.2 = album;
                    }
                } else {
                    on_progress_ref(line);
                }
            });
        });
        s.spawn(move || {
            stream_lines(stderr, |line| {
                on_progress_ref(line);
            });
        });
    });

    let status = child.wait().map_err(|e| format!("Failed to wait on engine: {}", e))?;

    if !status.success() {
        error!("Extraction failed with exit code {}", status);
        return Err(format!("Extraction failed with exit code {}", status));
    }

    info!("Extraction process finished successfully.");

    // Handle post-extraction Playlist / Plex ingestion if this was a custom playlist
    let (p_title, p_id, album) = playlist_meta.lock().unwrap().clone();
    let tracks = collected_tracks.lock().unwrap().clone();

    if is_custom_playlist(&p_id, &p_title, &album) && !tracks.is_empty() {
        on_progress(format!("Detected playlist '{}' ({} tracks). Generating M3U for Plex...", p_title, tracks.len()));
        
        let batch = PlaylistBatch {
            playlist_title: p_title,
            tracks,
        };

        let config = req.config.clone();
        // Run Plex ingestion in the Tokio async runtime
        let plex_msg = tauri::async_runtime::block_on(async {
            plex::handle_playlist_import(batch, &config).await
        });

        match plex_msg {
            Ok(msg) => {
                on_progress(format!("✅ {}", msg));
                return Ok(format!("Extraction and playlist import completed. {}", msg));
            }
            Err(e) => {
                on_progress(format!("⚠️ Plex playlist import note: {}", e));
            }
        }
    }

    Ok("Extraction completed successfully.".to_string())
}

/// Resolves the Engine binary location inside the app data directory.
pub fn engine_path(app: &AppHandle) -> Result<PathBuf, String> {
    let app_dir = app.path().app_data_dir().map_err(|e| {
        error!("Failed to resolve AppData directory: {}", e);
        "Failed to get app data dir".to_string()
    })?;
    Ok(app_dir.join("yt-dlp"))
}

/// GUI entry point: thin wrapper that serializes on the Engine Lock and forwards
/// progress lines to the frontend as `extraction-progress` events.
#[tauri::command]
pub async fn start_extraction(app: AppHandle, url: String) -> Result<String, String> {
    let engine_path = engine_path(&app)?;
    let app_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let config = crate::config::load_config(&app_dir);

    tauri::async_runtime::spawn_blocking(move || {
        let lock = app.state::<EngineLock>();
        let _guard = lock.acquire();
        let req = ExtractionRequest {
            engine_path: &engine_path,
            nas_root: Path::new(&config.mac_music_root),
            url: &url,
            config: &config,
        };
        extract(&req, |line| {
            let _ = app.emit("extraction-progress", line);
        })
    })
    .await
    .map_err(|e| format!("Extraction task panicked: {}", e))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    fn fixture(name: &str, script_body: &str) -> (PathBuf, PathBuf, PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "ytdlm-test-{}-{}-{}",
            name,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let nas = dir.join("nas");
        fs::create_dir_all(&nas).unwrap();
        let engine = dir.join("fake-yt-dlp");
        fs::write(&engine, format!("#!/bin/sh\n{}\n", script_body)).unwrap();
        fs::set_permissions(&engine, fs::Permissions::from_mode(0o755)).unwrap();
        (dir, nas, engine)
    }

    fn run(nas: &Path, engine: &Path, url: &str) -> (Result<String, String>, Vec<String>) {
        let lines = Arc::new(Mutex::new(Vec::new()));
        let lines_clone = lines.clone();
        let config = AppConfig::default();
        let req = ExtractionRequest {
            engine_path: engine,
            nas_root: nas,
            url,
            config: &config,
        };
        let result = extract(&req, move |l| lines_clone.lock().unwrap().push(l));
        let captured = lines.lock().unwrap().clone();
        (result, captured)
    }

    #[test]
    fn test_parse_track_delim_line() {
        let line = "YTDLM_TRACK:::/path/to/song.m4a:::My Title:::My Artist:::180.4:::My Playlist:::PL123:::NA";
        let (track, p_title, p_id, album) = parse_track_delim_line(line).unwrap();
        assert_eq!(track.filepath, "/path/to/song.m4a");
        assert_eq!(track.title, "My Title");
        assert_eq!(track.artist, "My Artist");
        assert_eq!(track.duration, 180);
        assert_eq!(p_title, "My Playlist");
        assert_eq!(p_id, "PL123");
        assert_eq!(album, "NA");
    }

    #[test]
    fn test_is_custom_playlist() {
        // Album from YouTube Music -> false
        assert!(!is_custom_playlist("OLAK5uy_abc123", "Some Album", "Some Album"));
        // Standalone song -> false
        assert!(!is_custom_playlist("NA", "NA", "NA"));
        // Custom playlist -> true
        assert!(is_custom_playlist("PL_12345", "Summer Vibes", "NA"));
        assert!(is_custom_playlist("PL_12345", "Summer Vibes", "Various"));
    }

    #[test]
    fn stream_lines_splits_on_cr_and_lf() {
        let mut out = Vec::new();
        stream_lines(&b"a\r\nb\rc\n\nd"[..], |l| out.push(l));
        assert_eq!(out, vec!["a", "b", "c", "d"]);
    }

    #[test]
    fn output_template_format() {
        assert!(OUTPUT_TEMPLATE.starts_with("%(artist,uploader|Unknown Artist)s/"));
        assert!(OUTPUT_TEMPLATE.ends_with("%(title)s.%(ext)s"));
    }

    #[test]
    fn success_streams_stdout_and_stderr_and_passes_url_last() {
        let (dir, nas, engine) = fixture(
            "ok",
            r#"for last; do :; done
printf 'url=%s\n' "$last"
printf '[download]  10%%\r[download]  50%%\r[download] 100%%\n'
echo 'WARNING: something' 1>&2
exit 0"#,
        );
        let (result, lines) = run(&nas, &engine, "https://music.youtube.com/watch?v=abc");
        assert_eq!(result, Ok("Extraction completed successfully.".to_string()));
        assert!(lines.contains(&"url=https://music.youtube.com/watch?v=abc".to_string()));
        assert!(lines.contains(&"[download]  50%".to_string()));
        assert!(lines.contains(&"[download] 100%".to_string()));
        assert!(lines.contains(&"WARNING: something".to_string()));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn nonzero_exit_is_an_error_and_stderr_is_still_reported() {
        let (dir, nas, engine) = fixture("fail", "echo 'ERROR: Video unavailable' 1>&2\nexit 1");
        let (result, lines) = run(&nas, &engine, "https://youtu.be/x");
        let err = result.unwrap_err();
        assert!(err.starts_with("Extraction failed with exit code"), "{err}");
        assert_eq!(lines, vec!["ERROR: Video unavailable"]);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn missing_nas_root_fails_before_running_engine() {
        let (dir, _nas, engine) = fixture("nonas", "echo should-not-run\nexit 0");
        let (result, lines) = run(&dir.join("not-mounted"), &engine, "https://youtu.be/x");
        assert!(result.unwrap_err().contains("NAS is not mounted"));
        assert!(lines.is_empty());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn missing_engine_is_reported() {
        let (dir, nas, _engine) = fixture("noengine", "exit 0");
        let (result, _) = run(&nas, &dir.join("missing"), "https://youtu.be/x");
        assert!(result.unwrap_err().starts_with("Engine not found"));
        fs::remove_dir_all(dir).unwrap();
    }
}
