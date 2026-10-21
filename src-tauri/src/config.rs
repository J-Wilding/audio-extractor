use std::fs;
use std::path::{Path, PathBuf};
use log::{error, info};
use serde::{Deserialize, Serialize};

pub const DEFAULT_PLEX_URL: &str = "http://10.0.0.69:32400";
pub const DEFAULT_SECTION_ID: &str = "3";
pub const DEFAULT_MAC_MUSIC_ROOT: &str = "/Volumes/data/Media/Music";
pub const DEFAULT_PLEX_MUSIC_ROOT: &str = "/share/CACHEDEV1_DATA/Multimedia/Media/Music";
pub const DEFAULT_PLAYLISTS_DIR: &str = "/Volumes/data/Media/Music/Playlists";

/// Normalizes a Plex URL, automatically prefixing `http://` if the user omitted a scheme.
pub fn normalize_plex_url(raw: &str) -> String {
    let trimmed = raw.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return DEFAULT_PLEX_URL.to_string();
    }
    if !trimmed.starts_with("http://") && !trimmed.starts_with("https://") {
        format!("http://{}", trimmed)
    } else {
        trimmed.to_string()
    }
}

const CONFIG_FILE: &str = "config.json";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default = "default_plex_url")]
    pub plex_url: String,
    #[serde(default = "default_section_id")]
    pub plex_section_id: String,
    #[serde(default)]
    pub plex_token: Option<String>,
    #[serde(default = "default_mac_music_root")]
    pub mac_music_root: String,
    #[serde(default = "default_plex_music_root")]
    pub plex_music_root: String,
    #[serde(default = "default_playlists_dir")]
    pub playlists_dir: String,
}

fn default_plex_url() -> String {
    DEFAULT_PLEX_URL.to_string()
}
fn default_section_id() -> String {
    DEFAULT_SECTION_ID.to_string()
}
fn default_mac_music_root() -> String {
    DEFAULT_MAC_MUSIC_ROOT.to_string()
}
fn default_plex_music_root() -> String {
    DEFAULT_PLEX_MUSIC_ROOT.to_string()
}
fn default_playlists_dir() -> String {
    DEFAULT_PLAYLISTS_DIR.to_string()
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            plex_url: default_plex_url(),
            plex_section_id: default_section_id(),
            plex_token: None,
            mac_music_root: default_mac_music_root(),
            plex_music_root: default_plex_music_root(),
            playlists_dir: default_playlists_dir(),
        }
    }
}

pub fn config_path(app_dir: &Path) -> PathBuf {
    app_dir.join(CONFIG_FILE)
}

pub fn load_config(app_dir: &Path) -> AppConfig {
    let path = config_path(app_dir);
    if path.exists() {
        match fs::read_to_string(&path) {
            Ok(raw) => {
                // First attempt: direct parse
                if let Ok(mut config) = serde_json::from_str::<AppConfig>(&raw) {
                    config.plex_url = normalize_plex_url(&config.plex_url);
                    return config;
                }

                // Second attempt: macOS TextEdit frequently replaces standard quotes with "smart quotes"
                let sanitized = raw
                    .replace('“', "\"")
                    .replace('”', "\"")
                    .replace('‘', "'")
                    .replace('’', "'");

                match serde_json::from_str::<AppConfig>(&sanitized) {
                    Ok(mut config) => {
                        config.plex_url = normalize_plex_url(&config.plex_url);
                        info!("Parsed config.json after normalizing smart quotes; saving clean version.");
                        let _ = save_config(app_dir, &config);
                        return config;
                    }
                    Err(e) => {
                        error!(
                            "Failed to parse existing config.json at {:?}: {}. Refusing to overwrite file with defaults.",
                            path, e
                        );
                        return AppConfig::default();
                    }
                }
            }
            Err(e) => {
                error!("Failed to read existing config.json at {:?}: {}", path, e);
                return AppConfig::default();
            }
        }
    }

    // Only create default config if the file does not exist at all
    let default = AppConfig::default();
    let _ = save_config(app_dir, &default);
    default
}

pub fn save_config(app_dir: &Path, config: &AppConfig) -> Result<(), String> {
    let mut normalized_config = config.clone();
    normalized_config.plex_url = normalize_plex_url(&normalized_config.plex_url);
    let path = config_path(app_dir);
    let json = serde_json::to_string_pretty(&normalized_config).map_err(|e| e.to_string())?;
    let _ = fs::create_dir_all(app_dir);
    fs::write(&path, json).map_err(|e| e.to_string())?;
    
    // Set 0600 permissions on unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
    }

    info!("Saved app configuration to {:?}", path);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_smart_quotes_normalization() {
        let dir = std::env::temp_dir().join(format!("test-config-quotes-{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        let path = dir.join(CONFIG_FILE);

        let smart_json = r#"{
  “plex_url”: “http://10.0.0.69:32400”,
  “plex_section_id”: “3”,
  “plex_token”: “my_secret_token”,
  “mac_music_root”: “/Volumes/data/Media/Music”,
  “plex_music_root”: “/share/CACHEDEV1_DATA/Multimedia/Media/Music”,
  “playlists_dir”: “/Volumes/data/Media/Music/Playlists”
}"#;
        fs::write(&path, smart_json).unwrap();

        let loaded = load_config(&dir);
        assert_eq!(loaded.plex_token.as_deref(), Some("my_secret_token"));
        assert_eq!(loaded.plex_url, "http://10.0.0.69:32400");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_corrupt_config_is_not_wiped() {
        let dir = std::env::temp_dir().join(format!("test-config-corrupt-{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        let path = dir.join(CONFIG_FILE);

        let broken_json = "{ broken json content that cannot be parsed }";
        fs::write(&path, broken_json).unwrap();

        let _ = load_config(&dir);
        // Verify file was NOT overwritten with default JSON
        let content_after = fs::read_to_string(&path).unwrap();
        assert_eq!(content_after, broken_json);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_normalize_plex_url() {
        assert_eq!(normalize_plex_url("10.0.0.69:32400"), "http://10.0.0.69:32400");
        assert_eq!(normalize_plex_url("10.0.0.69:32400/"), "http://10.0.0.69:32400");
        assert_eq!(normalize_plex_url("http://10.0.0.69:32400"), "http://10.0.0.69:32400");
        assert_eq!(normalize_plex_url("https://plex.myhouse.com:32400/"), "https://plex.myhouse.com:32400");
        assert_eq!(normalize_plex_url("   "), DEFAULT_PLEX_URL);
    }

    #[test]
    fn test_load_and_save_normalizes_url() {
        let dir = std::env::temp_dir().join(format!("test-config-norm-{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        let path = dir.join(CONFIG_FILE);

        let json = r#"{"plex_url": "10.0.0.69:32400"}"#;
        fs::write(&path, json).unwrap();

        let loaded = load_config(&dir);
        assert_eq!(loaded.plex_url, "http://10.0.0.69:32400");

        let mut cfg = AppConfig::default();
        cfg.plex_url = "192.168.1.50:32400".into();
        save_config(&dir, &cfg).unwrap();

        let reloaded = load_config(&dir);
        assert_eq!(reloaded.plex_url, "http://192.168.1.50:32400");

        let _ = fs::remove_dir_all(&dir);
    }
}
