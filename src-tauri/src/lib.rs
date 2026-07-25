mod engine;
mod extractor;
use std::sync::Mutex;
use tauri::Manager;

// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
#[tauri::command]
fn get_engine_status(state: tauri::State<engine::EngineState>) -> bool {
    *state.is_ready.lock().unwrap()
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
            
            // Kick off engine lifecycle in background
            engine::engine_setup(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_engine_status,
            extractor::start_extraction
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
