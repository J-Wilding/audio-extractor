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
            info!("yt-dlp executable found at {:?}. Skipping automatic update check.", engine_path);
        }

        // Update state and notify UI
        if let Some(state) = app.try_state::<EngineState>() {
            *state.is_ready.lock().unwrap() = true;
        }
        let _ = app.emit("engine-status", "Ready");
        info!("Engine lifecycle complete. Engine is locked and ready.");
    });
}

#[tauri::command]
pub async fn update_engine(app: AppHandle) -> Result<String, String> {
    let app_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let engine_path = app_dir.join("yt-dlp");
    
    if !engine_path.exists() {
        return Err("Engine not found. Restart app to download.".to_string());
    }
    
    info!("Running manual engine update...");
    info!("Executing command: {:?} -U", engine_path);
    
    let output = Command::new(&engine_path).arg("-U").output().map_err(|e| {
        error!("Failed to spawn update process: {}", e);
        e.to_string()
    })?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined_output = format!("{}\n{}", stdout, stderr).trim().to_string();

    if output.status.success() {
        info!("Manual engine update finished successfully. (Exit status 0)");
        info!("yt-dlp output:\n{}", combined_output);
        Ok(combined_output)
    } else {
        error!("Manual engine update failed with status: {}", output.status);
        error!("yt-dlp output:\n{}", combined_output);
        Err(combined_output)
    }
}
