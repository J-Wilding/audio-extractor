use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;

use log::{error, info};

use audio_extractor_server::config::Config;
use audio_extractor_server::engine::{self, Engine};
use audio_extractor_server::jobs::JobStore;
use audio_extractor_server::worker::{run_worker, AppState};
use audio_extractor_server::api;

#[tokio::main]
async fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let cfg = match Config::from_env() {
        Ok(c) => Arc::new(c),
        Err(e) => {
            error!("Configuration error: {}", e);
            std::process::exit(2);
        }
    };
    info!("Starting Audio Extractor server v{} with {:?}", env!("CARGO_PKG_VERSION"), cfg);
    if !cfg.music_root.is_dir() {
        // Not fatal: the mount may appear later. Jobs fail clearly until it does.
        error!("MUSIC_ROOT {:?} is not a directory. Check the volume mount.", cfg.music_root);
    }

    let engine = Arc::new(Engine::new(&cfg));
    let jobs = Arc::new(JobStore::load(cfg.data_dir.join("jobs.json")));
    let state = AppState { cfg: cfg.clone(), engine: engine.clone(), jobs };

    tokio::spawn(engine::bootstrap(engine, cfg.data_dir.clone()));
    tokio::spawn(run_worker(state.clone()));

    let addr = SocketAddr::from((Ipv4Addr::UNSPECIFIED, cfg.port));
    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) => {
            error!("Failed to bind {}: {}", addr, e);
            std::process::exit(1);
        }
    };
    info!("Listening on http://{}", addr);
    if let Err(e) = axum::serve(listener, api::router(state))
        .with_graceful_shutdown(shutdown_signal())
        .await
    {
        error!("Server error: {}", e);
        std::process::exit(1);
    }
    info!("Shut down.");
}

/// Resolves on Ctrl-C or SIGTERM (`docker stop`).
async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    let term = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut s) => {
                s.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };
    tokio::select! {
        _ = ctrl_c => {},
        _ = term => {},
    }
    info!("Shutdown signal received.");
}
