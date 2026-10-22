//! Server configuration, read entirely from environment variables (see spec v2).
//! Secrets (`BRIDGE_TOKEN`, `PLEX_TOKEN`) are never printed: `Debug` redacts them.

use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;

pub const DEFAULT_PORT: u16 = 47821;
pub const DEFAULT_PLEX_URL: &str = "http://10.0.0.69:32400";
pub const DEFAULT_SECTION_ID: &str = "3";
pub const DEFAULT_MUSIC_ROOT: &str = "/music";
pub const DEFAULT_PLEX_MUSIC_ROOT: &str = "/share/CACHEDEV1_DATA/Multimedia/Media/Music";
pub const DEFAULT_DATA_DIR: &str = "/data";
/// Short tokens are trivially brute-forced on a LAN; insist on something real.
pub const MIN_TOKEN_LEN: usize = 24;

/// Everything the Plex step needs.
#[derive(Clone)]
pub struct PlexConfig {
    /// Always has an `http(s)://` scheme and no trailing slash (see [`normalize_plex_url`]).
    pub url: String,
    pub token: Option<String>,
    pub section_id: String,
    /// Music root as this process sees it (e.g. `/music` in the container).
    pub local_music_root: String,
    /// The same directory as the Plex server sees it.
    pub plex_music_root: String,
    pub playlists_dir: PathBuf,
}

#[derive(Clone)]
pub struct Config {
    pub port: u16,
    pub bridge_token: String,
    pub music_root: PathBuf,
    pub data_dir: PathBuf,
    /// Explicit Engine binary. When unset, the server manages `DATA_DIR/yt-dlp` itself.
    pub engine_path: Option<PathBuf>,
    pub plex: PlexConfig,
}

impl Config {
    /// Reads the configuration from the process environment.
    pub fn from_env() -> Result<Self, String> {
        Self::from_map(&std::env::vars().collect())
    }

    /// Testable core of [`Config::from_env`].
    pub fn from_map(env: &HashMap<String, String>) -> Result<Self, String> {
        let get = |k: &str| env.get(k).map(|v| v.trim().to_string()).filter(|v| !v.is_empty());

        let bridge_token = get("BRIDGE_TOKEN").ok_or(
            "BRIDGE_TOKEN is required. Generate one with: openssl rand -hex 24",
        )?;
        if bridge_token.len() < MIN_TOKEN_LEN {
            return Err(format!(
                "BRIDGE_TOKEN must be at least {} characters. Generate one with: openssl rand -hex 24",
                MIN_TOKEN_LEN
            ));
        }

        let port = match get("PORT") {
            Some(p) => p.parse().map_err(|_| format!("PORT must be a number, got {:?}", p))?,
            None => DEFAULT_PORT,
        };

        let music_root = PathBuf::from(get("MUSIC_ROOT").unwrap_or_else(|| DEFAULT_MUSIC_ROOT.into()));
        let data_dir = PathBuf::from(get("DATA_DIR").unwrap_or_else(|| DEFAULT_DATA_DIR.into()));
        let playlists_dir = get("PLAYLISTS_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| music_root.join("Playlists"));

        let plex = PlexConfig {
            url: normalize_plex_url(&get("PLEX_URL").unwrap_or_default()),
            token: get("PLEX_TOKEN"),
            section_id: get("PLEX_SECTION_ID").unwrap_or_else(|| DEFAULT_SECTION_ID.into()),
            local_music_root: music_root.to_string_lossy().into_owned(),
            plex_music_root: get("PLEX_MUSIC_ROOT").unwrap_or_else(|| DEFAULT_PLEX_MUSIC_ROOT.into()),
            playlists_dir,
        };

        Ok(Self {
            port,
            bridge_token,
            music_root,
            data_dir,
            engine_path: get("ENGINE_PATH").map(PathBuf::from),
            plex,
        })
    }
}

/// Normalizes a Plex URL, prefixing `http://` if the user omitted a scheme.
pub fn normalize_plex_url(raw: &str) -> String {
    let trimmed = raw.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        DEFAULT_PLEX_URL.to_string()
    } else if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        trimmed.to_string()
    } else {
        format!("http://{}", trimmed)
    }
}

fn redact(v: &Option<String>) -> &'static str {
    if v.is_some() { "<set>" } else { "<unset>" }
}

impl fmt::Debug for PlexConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PlexConfig")
            .field("url", &self.url)
            .field("token", &redact(&self.token))
            .field("section_id", &self.section_id)
            .field("local_music_root", &self.local_music_root)
            .field("plex_music_root", &self.plex_music_root)
            .field("playlists_dir", &self.playlists_dir)
            .finish()
    }
}

impl fmt::Debug for Config {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Config")
            .field("port", &self.port)
            .field("bridge_token", &"<set>")
            .field("music_root", &self.music_root)
            .field("data_dir", &self.data_dir)
            .field("engine_path", &self.engine_path)
            .field("plex", &self.plex)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOKEN: &str = "0123456789abcdef0123456789abcdef";

    fn env(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn token_is_required_and_must_be_long() {
        assert!(Config::from_map(&env(&[])).unwrap_err().contains("BRIDGE_TOKEN is required"));
        assert!(Config::from_map(&env(&[("BRIDGE_TOKEN", "short")])).unwrap_err().contains("at least"));
    }

    #[test]
    fn defaults() {
        let c = Config::from_map(&env(&[("BRIDGE_TOKEN", TOKEN)])).unwrap();
        assert_eq!(c.port, DEFAULT_PORT);
        assert_eq!(c.music_root, PathBuf::from("/music"));
        assert_eq!(c.plex.playlists_dir, PathBuf::from("/music/Playlists"));
        assert_eq!(c.plex.url, DEFAULT_PLEX_URL);
        assert_eq!(c.plex.section_id, "3");
        assert!(c.plex.token.is_none());
        assert!(c.engine_path.is_none());
    }

    #[test]
    fn overrides_and_url_normalization() {
        let c = Config::from_map(&env(&[
            ("BRIDGE_TOKEN", TOKEN),
            ("PORT", "8080"),
            ("PLEX_URL", "10.0.0.69:32400/"),
            ("PLEX_TOKEN", "plex-secret"),
            ("MUSIC_ROOT", "/mnt/music"),
            ("PLAYLISTS_DIR", "/mnt/pl"),
        ]))
        .unwrap();
        assert_eq!(c.port, 8080);
        assert_eq!(c.plex.url, "http://10.0.0.69:32400");
        assert_eq!(c.plex.local_music_root, "/mnt/music");
        assert_eq!(c.plex.playlists_dir, PathBuf::from("/mnt/pl"));
    }

    #[test]
    fn bad_port_is_rejected() {
        assert!(Config::from_map(&env(&[("BRIDGE_TOKEN", TOKEN), ("PORT", "abc")])).is_err());
    }

    #[test]
    fn debug_never_prints_secrets() {
        let c = Config::from_map(&env(&[("BRIDGE_TOKEN", TOKEN), ("PLEX_TOKEN", "plex-secret")])).unwrap();
        let dbg = format!("{:?}", c);
        assert!(!dbg.contains(TOKEN));
        assert!(!dbg.contains("plex-secret"));
    }

    #[test]
    fn normalize_plex_url_cases() {
        assert_eq!(normalize_plex_url("10.0.0.69:32400"), "http://10.0.0.69:32400");
        assert_eq!(normalize_plex_url("https://plex.example:32400/"), "https://plex.example:32400");
        assert_eq!(normalize_plex_url("  "), DEFAULT_PLEX_URL);
    }
}
