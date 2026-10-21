mod bridge;
mod config;
mod engine;
mod extractor;
mod plex;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use log::error;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
#[tauri::command]
fn get_engine_status(state: tauri::State<engine::EngineState>) -> bool {
    *state.is_ready.lock().unwrap()
}

#[tauri::command]
fn resize_window(app: tauri::AppHandle, width: f64, height: f64) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.set_size(tauri::Size::Logical(tauri::LogicalSize { width, height }));
    }
}

/// Connection details the user pastes into the browser extension's options page.
#[derive(Clone, Serialize)]
struct BridgeInfo {
    port: u16,
    token: Option<String>,
    /// `None` while healthy; otherwise why the Bridge isn't available.
    error: Option<String>,
}

#[tauri::command]
fn get_bridge_info(state: tauri::State<Mutex<BridgeInfo>>) -> BridgeInfo {
    state.lock().unwrap().clone()
}

#[tauri::command]
async fn trigger_plex_scan(app: AppHandle) -> Result<String, String> {
    let app_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let conf = config::load_config(&app_dir);
    if conf.plex_token.is_none() {
        return Err("Plex token is not configured. Please set your token in settings.".to_string());
    }
    plex::trigger_plex_refresh(&conf).await?;
    Ok("Plex library scan triggered successfully.".to_string())
}

#[tauri::command]
fn get_app_config(app: AppHandle) -> Result<config::AppConfig, String> {
    let app_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    Ok(config::load_config(&app_dir))
}

#[tauri::command]
fn save_app_config(app: AppHandle, config: config::AppConfig) -> Result<String, String> {
    let app_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    config::save_config(&app_dir, &config)?;
    Ok("Configuration saved successfully.".to_string())
}

/// Connects the Browser Bridge to the running Tauri app.
struct TauriBridgeHost {
    app: AppHandle,
    engine_path: PathBuf,
}

impl bridge::BridgeHost for TauriBridgeHost {
    fn engine_ready(&self) -> bool {
        *self.app.state::<engine::EngineState>().is_ready.lock().unwrap()
    }
    fn engine_path(&self) -> PathBuf {
        self.engine_path.clone()
    }
    fn nas_root(&self) -> PathBuf {
        let app_dir = self.app.path().app_data_dir().unwrap_or_default();
        let conf = config::load_config(&app_dir);
        PathBuf::from(conf.mac_music_root)
    }
    fn config(&self) -> config::AppConfig {
        let app_dir = self.app.path().app_data_dir().unwrap_or_default();
        config::load_config(&app_dir)
    }
    fn on_progress(&self, line: String) {
        // Same event the GUI's own jobs use, so the progress bar just works.
        let _ = self.app.emit("extraction-progress", line);
    }
    fn on_job_update(&self, job: &bridge::Job) {
        let _ = self.app.emit("bridge-job", job);
    }
}

/// Starts the Browser Bridge in the background. Failures (e.g. port in use) are
/// recorded in `BridgeInfo` for the UI instead of crashing the app.
fn start_bridge(app: &AppHandle) {
    let port = bridge::configured_port();
    let info = app.state::<Mutex<BridgeInfo>>();
    let set_error = |msg: String| {
        error!("{}", msg);
        info.lock().unwrap().error = Some(msg);
    };

    let (app_dir, engine_path) = match (app.path().app_data_dir(), extractor::engine_path(app)) {
        (Ok(dir), Ok(engine)) => (dir, engine),
        _ => return set_error("Browser Bridge disabled: could not resolve app data dir.".into()),
    };
    let token = match bridge::load_or_create_token(&app_dir) {
        Ok(t) => t,
        Err(e) => return set_error(format!("Browser Bridge disabled: token error: {}", e)),
    };
    {
        let mut info = info.lock().unwrap();
        info.port = port;
        info.token = Some(token.clone());
    }

    let host = Arc::new(TauriBridgeHost { app: app.clone(), engine_path });
    let state = bridge::BridgeState::new(token, app.state::<engine::EngineLock>().inner().clone(), host);
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = bridge::serve(state, port).await {
            let msg = format!("Browser Bridge stopped: {} (port {})", e, port);
            error!("{}", msg);
            app.state::<Mutex<BridgeInfo>>().lock().unwrap().error = Some(msg);
        }
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            // Initialize engine state as NOT ready
            app.manage(engine::EngineState {
                is_ready: Mutex::new(false),
            });
            // Must be managed before engine_setup, which takes it immediately.
            app.manage(engine::EngineLock::default());
            app.manage(Mutex::new(BridgeInfo {
                port: bridge::configured_port(),
                token: None,
                error: None,
            }));
            
            // Kick off engine lifecycle in background
            engine::engine_setup(app.handle().clone());
            start_bridge(app.handle());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_engine_status,
            get_bridge_info,
            extractor::start_extraction,
            engine::update_engine,
            resize_window,
            trigger_plex_scan,
            get_app_config,
            save_app_config
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
