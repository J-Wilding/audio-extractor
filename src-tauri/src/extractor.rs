use std::path::Path;
use std::process::Command;
use tauri::{AppHandle, Manager};
use log::{info, error, debug, warn};

#[tauri::command]
pub fn start_extraction(app: AppHandle, url: String) -> Result<String, String> {
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

    let result = Command::new(&engine_path)
        .arg("-x")
        .arg("--audio-format")
        .arg("m4a")
        .arg("--embed-metadata")
        .arg("-o")
        .arg(output_template)
        .arg(&url)
        .output();

    match result {
        Ok(output) => {
            if output.status.success() {
                info!("Extraction process finished successfully.");
                Ok("Extraction completed successfully.".to_string())
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr);
                error!("Extraction failed with exit code {}: \n{}", output.status, stderr);
                Err(format!("Extraction failed:\n{}", stderr))
            }
        }
        Err(e) => {
            error!("Failed to even spawn the engine subprocess: {}", e);
            Err(format!("Failed to spawn engine: {}", e))
        }
    }
}
