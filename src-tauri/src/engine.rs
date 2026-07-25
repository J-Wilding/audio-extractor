use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Mutex;
use tauri::{AppHandle, Manager, Emitter};
use std::os::unix::fs::PermissionsExt;
use log::{info, debug, error};

pub struct EngineState {
    pub is_ready: Mutex<bool>,
}

pub fn engine_setup(app: AppHandle) {
    std::thread::spawn(move || {
        // Emit initial updating status
        let _ = app.emit("engine-status", "Updating Engine...");

        let app_dir = app.path().app_data_dir().expect("Failed to get app data dir");
        debug!("App data directory resolved to: {:?}", app_dir);

        if !app_dir.exists() {
            info!("App data directory missing, creating it...");
            fs::create_dir_all(&app_dir).unwrap();
        }

        let engine_path = app_dir.join("yt-dlp");
        debug!("Target engine path: {:?}", engine_path);

        if !engine_path.exists() {
            info!("yt-dlp executable not found. Starting download from GitHub...");
            if let Ok(mut response) = reqwest::blocking::get("https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_macos") {
                if let Ok(mut file) = fs::File::create(&engine_path) {
                    let _ = std::io::copy(&mut response, &mut file);
                    info!("Download complete. Setting executable permissions...");
                    // Make executable
                    if let Ok(metadata) = fs::metadata(&engine_path) {
                        let mut perms = metadata.permissions();
                        perms.set_mode(0o755);
                        let _ = fs::set_permissions(&engine_path, perms);
                        info!("Permissions set successfully.");
                    }
                } else {
                    error!("Failed to create file for yt-dlp binary.");
                }
            } else {
                error!("Network request to download yt-dlp failed.");
            }
        } else {
            info!("yt-dlp executable found at {:?}. Checking for updates...", engine_path);
            let status = Command::new(&engine_path).arg("-U").status();
            match status {
                Ok(s) => info!("yt-dlp update process finished with status: {}", s),
                Err(e) => error!("Failed to run yt-dlp update: {}", e),
            }
        }

        // Update state and notify UI
        if let Some(state) = app.try_state::<EngineState>() {
            *state.is_ready.lock().unwrap() = true;
        }
        let _ = app.emit("engine-status", "Ready");
        info!("Engine lifecycle complete. Engine is locked and ready.");
    });
}
