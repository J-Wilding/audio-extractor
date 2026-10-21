use std::fs;
use std::path::{Path, PathBuf};
use log::{info, warn};
use serde::{Deserialize, Serialize};

pub const DEFAULT_PLEX_URL: &str = "http://10.0.0.69:32400";
pub const DEFAULT_SECTION_ID: &str = "3";
pub const DEFAULT_MAC_MUSIC_ROOT: &str = "/Volumes/data/Media/Music";
pub const DEFAULT_PLEX_MUSIC_ROOT: &str = "/share/CACHEDEV1_DATA/Multimedia/Media/Music";
pub const DEFAULT_PLAYLISTS_DIR: &str = "/Volumes/data/Media/Music/Playlists";

const CONFIG_FILE: &str = "config.json";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppConfig {
    pub plex_url: String,
    pub plex_section_id: String,
    pub plex_token: Option<String>,
    pub mac_music_root: String,
    pub plex_music_root: String,
    pub playlists_dir: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            plex_url: DEFAULT_PLEX_URL.to_string(),
            plex_section_id: DEFAULT_SECTION_ID.to_string(),
            plex_token: None,
            mac_music_root: DEFAULT_MAC_MUSIC_ROOT.to_string(),
            plex_music_root: DEFAULT_PLEX_MUSIC_ROOT.to_string(),
            playlists_dir: DEFAULT_PLAYLISTS_DIR.to_string(),
        }
    }
}

pub fn config_path(app_dir: &Path) -> PathBuf {
    app_dir.join(CONFIG_FILE)
}

pub fn load_config(app_dir: &Path) -> AppConfig {
    let path = config_path(app_dir);
    if let Ok(data) = fs::read_to_string(&path) {
        if let Ok(config) = serde_json::from_str::<AppConfig>(&data) {
            return config;
        }
    }
    let default = AppConfig::default();
    let _ = save_config(app_dir, &default);
    default
}

pub fn save_config(app_dir: &Path, config: &AppConfig) -> Result<(), String> {
    let path = config_path(app_dir);
    let json = serde_json::to_string_pretty(config).map_err(|e| e.to_string())?;
    fs::write(&path, json).map_err(|e| e.to_string())?;
    info!("Saved app configuration to {:?}", path);
    Ok(())
}
