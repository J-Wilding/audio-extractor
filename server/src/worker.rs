//! The single job worker: takes queued jobs one at a time, runs the Engine under
//! the Engine Lock, then does the Plex step for custom playlists.

use std::sync::Arc;
use std::time::Duration;

use log::error;

use crate::config::Config;
use crate::engine::Engine;
use crate::extractor::{extract, Event, ExtractError, ExtractionRequest, Outcome};
use crate::jobs::{Claim, JobStatus, JobStore};
use crate::plex;

/// Shared server state.
#[derive(Clone)]
pub struct AppState {
    pub cfg: Arc<Config>,
    pub engine: Arc<Engine>,
    pub jobs: Arc<JobStore>,
}

/// Runs forever, processing jobs in FIFO order.
pub async fn run_worker(state: AppState) {
    loop {
        // Leave jobs queued (not failed) while the Engine is still being installed.
        while !state.engine.is_ready() {
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        match state.jobs.claim_next() {
            Some(claim) => run_job(&state, claim).await,
            None => state.jobs.wait_for_work().await,
        }
    }
}

/// Runs one claimed job to completion and records the result.
pub async fn run_job(state: &AppState, claim: Claim) {
    let id = claim.id;
    let result = {
        let st = state.clone();
        tokio::task::spawn_blocking(move || {
            let _guard = st.engine.lock.acquire();
            let temp_dir = st.cfg.data_dir.join("tmp");
            let req = ExtractionRequest {
                engine_path: &st.engine.path,
                music_root: &st.cfg.music_root,
                temp_dir: &temp_dir,
                url: &claim.url,
                mode: claim.mode,
                cancel: &claim.cancel,
            };
            extract(&req, |event| match event {
                Event::Line(line) => st.jobs.append_line(id, line),
                Event::Track { track, playlist_title } => st.jobs.record_track(id, &track, &playlist_title),
            })
        })
        .await
        .unwrap_or_else(|e| {
            error!("Job {} panicked: {}", id, e);
            Err(ExtractError::Failed(format!("Internal error: {}", e)))
        })
    };

    match result {
        Ok(outcome) => {
            let message = finish_success(state, id, outcome).await;
            state.jobs.finish(id, JobStatus::Succeeded, message);
        }
        Err(ExtractError::Cancelled) => state.jobs.finish(id, JobStatus::Cancelled, "Cancelled.".into()),
        Err(ExtractError::Failed(msg)) => state.jobs.finish(id, JobStatus::Failed, msg),
    }
}

/// Runs the Plex step if needed and builds the success summary. Plex problems
/// are reported in the message; they never fail the job.
async fn finish_success(state: &AppState, id: u64, outcome: Outcome) -> String {
    let n = outcome.tracks.len();
    let saved = if n == 1 { "Saved 1 track.".to_string() } else { format!("Saved {} tracks.", n) };
    let Some(batch) = outcome.playlist else {
        // Trigger a non-blocking Plex scan for newly downloaded songs/albums when configured.
        if n > 0 && state.cfg.plex.token.is_some() {
            if let Err(e) = plex::trigger_plex_refresh(&state.cfg.plex).await {
                state.jobs.append_line(id, format!("⚠️ Plex scan note: {}", e));
            } else {
                state.jobs.append_line(id, "Triggered Plex library scan.".into());
            }
        }
        return saved;
    };

    state.jobs.append_line(
        id,
        format!("Detected playlist '{}' ({} tracks). Generating M3U for Plex…", batch.playlist_title, batch.tracks.len()),
    );
    match plex::handle_playlist_import(batch, &state.cfg.plex).await {
        Ok(msg) => {
            state.jobs.append_line(id, msg.clone());
            format!("{} {}", saved, msg)
        }
        Err(e) => {
            state.jobs.append_line(id, format!("⚠️ Playlist file not written: {}", e));
            format!("{} (Playlist file failed: {})", saved, e)
        }
    }
}
