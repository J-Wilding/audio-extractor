use std::path::Path;
use std::process::{Command, Stdio};
use tauri::{AppHandle, Manager, Emitter};
use log::{info, error, debug, warn};

#[tauri::command]
pub async fn start_extraction(app: AppHandle, url: String) -> Result<String, String> {
    let nas_root = Path::new("/Volumes/data/Media/Music/");
    debug!("Validating NAS Root at {:?}", nas_root);
    if !nas_root.exists() {
        warn!("NAS Validation Failed: Directory does not exist.");
        return Err("NAS is not mounted or accessible at /Volumes/data/Media/Music/. Please mount your NAS first.".to_string());
    }

    let app_dir = app.path().app_data_dir().map_err(|e| {
        error!("Failed to resolve AppData directory: {}", e);
        "Failed to get app data dir".to_string()
    })?;
    let engine_path = app_dir.join("yt-dlp");
    debug!("Using Engine binary at {:?}", engine_path);

    if !engine_path.exists() {
        error!("Engine binary is completely missing at runtime.");
        return Err("Engine not found. Please wait for it to download or restart the app.".to_string());
    }

    let output_template = "/Volumes/data/Media/Music/%(artist|uploader)s/%(album|playlist|\"Singles\")s/%(playlist_index|track_number)s - %(title)s.%(ext)s";

    info!("Starting extraction process for URL: {}", url);
    debug!("Using Output Template: {}", output_template);

    // GUI apps on macOS don't inherit the user's shell PATH, so Homebrew binaries are often missing.
    // We inject the standard Apple Silicon and Intel Homebrew paths so yt-dlp can find ffmpeg.
    let current_path = std::env::var("PATH").unwrap_or_default();
    let new_path = format!("{}:/opt/homebrew/bin:/usr/local/bin", current_path);

    let mut child = Command::new(&engine_path)
        .env("PATH", new_path)
        .arg("-x")
        .arg("--audio-format")
        .arg("m4a")
        .arg("--embed-metadata")
        .arg("-o")
        .arg(output_template)
        .arg(&url)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            error!("Failed to even spawn the engine subprocess: {}", e);
            format!("Failed to spawn engine: {}", e)
        })?;

    let stdout = child.stdout.take().expect("Failed to open stdout");
    let stderr = child.stderr.take().expect("Failed to open stderr");

    let app_clone1 = app.clone();
    let stdout_handle = std::thread::spawn(move || {
        use std::io::{Read, BufReader};
        let mut reader = BufReader::new(stdout);
        let mut buf = Vec::new();
        let mut buffer = [0; 1];
        while let Ok(n) = reader.read(&mut buffer) {
            if n == 0 {
                break;
            }
            let c = buffer[0];
            if c == b'\n' || c == b'\r' {
                if !buf.is_empty() {
                    let s = String::from_utf8_lossy(&buf).to_string();
                    let _ = app_clone1.emit("extraction-progress", s);
                    buf.clear();
                }
            } else {
                buf.push(c);
            }
        }
        if !buf.is_empty() {
            let s = String::from_utf8_lossy(&buf).to_string();
            let _ = app_clone1.emit("extraction-progress", s);
        }
    });

    let app_clone2 = app.clone();
    let stderr_handle = std::thread::spawn(move || {
        use std::io::{Read, BufReader};
        let mut reader = BufReader::new(stderr);
        let mut buf = Vec::new();
        let mut buffer = [0; 1];
        while let Ok(n) = reader.read(&mut buffer) {
            if n == 0 {
                break;
            }
            let c = buffer[0];
            if c == b'\n' || c == b'\r' {
                if !buf.is_empty() {
                    let s = String::from_utf8_lossy(&buf).to_string();
                    let _ = app_clone2.emit("extraction-progress", s);
                    buf.clear();
                }
            } else {
                buf.push(c);
            }
        }
        if !buf.is_empty() {
            let s = String::from_utf8_lossy(&buf).to_string();
            let _ = app_clone2.emit("extraction-progress", s);
        }
    });

    let status = child.wait().map_err(|e| format!("Failed to wait on engine: {}", e))?;
    let _ = stdout_handle.join();
    let _ = stderr_handle.join();

    if status.success() {
        info!("Extraction process finished successfully.");
        Ok("Extraction completed successfully.".to_string())
    } else {
        error!("Extraction failed with exit code {}", status);
        Err(format!("Extraction failed with exit code {}", status))
    }
}
