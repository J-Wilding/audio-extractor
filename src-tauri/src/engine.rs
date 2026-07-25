use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Mutex;
use tauri::{AppHandle, Manager, Emitter};
use std::os::unix::fs::PermissionsExt;

pub struct EngineState {
    pub is_ready: Mutex<bool>,
}

pub fn engine_setup(app: AppHandle) {
    std::thread::spawn(move || {
        // Emit initial updating status
        let _ = app.emit("engine-status", "Updating Engine...");

        let app_dir = app.path().app_data_dir().expect("Failed to get app data dir");
        if !app_dir.exists() {
            fs::create_dir_all(&app_dir).unwrap();
        }

        let engine_path = app_dir.join("yt-dlp");

        if !engine_path.exists() {
            println!("yt-dlp not found, downloading...");
            if let Ok(mut response) = reqwest::blocking::get("https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_macos") {
                if let Ok(mut file) = fs::File::create(&engine_path) {
                    let _ = std::io::copy(&mut response, &mut file);
                    
                    // Make executable
                    if let Ok(metadata) = fs::metadata(&engine_path) {
                        let mut perms = metadata.permissions();
                        perms.set_mode(0o755);
                        let _ = fs::set_permissions(&engine_path, perms);
                    }
                }
            }
        } else {
            println!("yt-dlp found, updating...");
            let _ = Command::new(&engine_path).arg("-U").status();
        }

        // Update state and notify UI
        if let Some(state) = app.try_state::<EngineState>() {
            *state.is_ready.lock().unwrap() = true;
        }
        let _ = app.emit("engine-status", "Ready");
        println!("Engine ready.");
    });
}
