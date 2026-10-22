//! Engine lifecycle (see CONTEXT.md): download yt-dlp on first start, keep it
//! updated, and serialize everything that touches it through the Engine Lock.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use log::{error, info, warn};

use crate::config::Config;

/// How often a managed Engine runs `yt-dlp -U`.
pub const UPDATE_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

/// Held while the Engine is being downloaded or updated, and for the duration of
/// every extraction job. Cheap to clone; clones share the same lock.
#[derive(Default, Clone)]
pub struct EngineLock(Arc<Mutex<()>>);

impl EngineLock {
    /// Blocks until the lock is free. Poisoning is ignored: the guarded data is `()`.
    pub fn acquire(&self) -> MutexGuard<'_, ()> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Shared Engine state.
pub struct Engine {
    pub path: PathBuf,
    /// `false` when `ENGINE_PATH` points at an externally managed binary.
    pub managed: bool,
    pub lock: EngineLock,
    ready: AtomicBool,
    version: Mutex<Option<String>>,
}

impl Engine {
    pub fn new(cfg: &Config) -> Self {
        let (path, managed) = match &cfg.engine_path {
            Some(p) => (p.clone(), false),
            None => (cfg.data_dir.join("yt-dlp"), true),
        };
        Self {
            path,
            managed,
            lock: EngineLock::default(),
            ready: AtomicBool::new(false),
            version: Mutex::new(None),
        }
    }

    pub fn is_ready(&self) -> bool {
        self.ready.load(Ordering::Acquire)
    }

    pub fn version(&self) -> Option<String> {
        self.version.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    /// For tests and externally managed Engines.
    pub fn mark_ready(&self) {
        self.ready.store(true, Ordering::Release);
    }

    fn refresh_version(&self) {
        let v = Command::new(&self.path)
            .arg("--version")
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());
        *self.version.lock().unwrap_or_else(PoisonError::into_inner) = v;
    }
}

/// The yt-dlp release asset for this platform.
pub fn release_asset() -> &'static str {
    if cfg!(target_os = "macos") {
        "yt-dlp_macos"
    } else if cfg!(target_arch = "aarch64") {
        "yt-dlp_linux_aarch64"
    } else {
        "yt-dlp_linux"
    }
}

async fn download_engine(path: &Path) -> Result<(), String> {
    let url = format!("https://github.com/yt-dlp/yt-dlp/releases/latest/download/{}", release_asset());
    info!("Downloading Engine from {}", url);
    let resp = reqwest::get(&url).await.map_err(|e| format!("Engine download failed: {}", e))?;
    if !resp.status().is_success() {
        return Err(format!("Engine download failed: HTTP {}", resp.status()));
    }
    let bytes = resp.bytes().await.map_err(|e| format!("Engine download failed: {}", e))?;

    // Write next to the target and rename, so a crash never leaves a half-written Engine.
    let tmp = path.with_extension("download");
    std::fs::write(&tmp, &bytes).map_err(|e| format!("Failed to write Engine: {}", e))?;
    std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))
        .map_err(|e| format!("Failed to chmod Engine: {}", e))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("Failed to install Engine: {}", e))?;
    info!("Engine installed at {:?} ({} bytes)", path, bytes.len());
    Ok(())
}

/// Runs `yt-dlp -U` while holding the Engine Lock (waits for any running job).
pub async fn update_engine(engine: Arc<Engine>) -> Result<String, String> {
    tokio::task::spawn_blocking(move || {
        let _guard = engine.lock.acquire();
        info!("Updating Engine ({:?} -U)", engine.path);
        let out = Command::new(&engine.path)
            .arg("-U")
            .output()
            .map_err(|e| format!("Failed to run Engine update: {}", e))?;
        let text = format!(
            "{}\n{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        )
        .trim()
        .to_string();
        engine.refresh_version();
        if out.status.success() {
            info!("Engine update finished: {}", text);
            Ok(text)
        } else {
            Err(text)
        }
    })
    .await
    .map_err(|e| format!("Engine update task panicked: {}", e))?
}

/// Brings the Engine up (download if needed, then update) and schedules daily
/// updates. Runs in the background; jobs wait on `is_ready()` / the Engine Lock.
pub async fn bootstrap(engine: Arc<Engine>, data_dir: PathBuf) {
    if !engine.managed {
        if engine.path.exists() {
            engine.refresh_version();
            info!("Using external Engine at {:?} ({:?})", engine.path, engine.version());
            engine.mark_ready();
        } else {
            error!("ENGINE_PATH {:?} does not exist; jobs will fail.", engine.path);
        }
        return;
    }

    if let Err(e) = std::fs::create_dir_all(&data_dir) {
        error!("Cannot create DATA_DIR {:?}: {}", data_dir, e);
    }

    // Retry the first download until it works; nothing can run without an Engine.
    let mut backoff = Duration::from_secs(5);
    while !engine.path.exists() {
        // No lock needed: the worker doesn't start jobs until `is_ready()`, and
        // the download is installed with an atomic rename.
        match download_engine(&engine.path).await {
            Ok(()) => break,
            Err(e) => {
                error!("{} (retrying in {:?})", e, backoff);
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(Duration::from_secs(300));
            }
        }
    }

    if let Err(e) = update_engine(engine.clone()).await {
        warn!("Engine update failed (continuing with current version): {}", e);
    }
    engine.mark_ready();
    info!("Engine ready: {:?}", engine.version());

    loop {
        tokio::time::sleep(UPDATE_INTERVAL).await;
        if let Err(e) = update_engine(engine.clone()).await {
            warn!("Scheduled Engine update failed: {}", e);
        }
    }
}
