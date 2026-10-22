//! Job queue and Job Log (see spec v2, issue 14).
//!
//! Every Engine line is stored with a monotonically increasing `seq`, so clients
//! can read the log incrementally (`since=<seq>`) and catch up after reconnecting.
//! Finished jobs (with their logs) are persisted to `jobs.json` so a restart
//! doesn't wipe history.

use std::collections::{BTreeMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

use log::{error, info, warn};
use serde::{Deserialize, Serialize};
use tokio::sync::Notify;

use crate::extractor::Mode;
use crate::plex::TrackMetadata;

/// Lines kept per job; older lines are dropped (their `seq`s are never reused).
pub const MAX_LOG_LINES: usize = 2000;
/// Finished jobs kept in memory and on disk.
pub const MAX_HISTORY: usize = 50;
const PERSIST_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JobStatus {
    Queued,
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

impl JobStatus {
    pub fn is_finished(self) -> bool {
        matches!(self, JobStatus::Succeeded | JobStatus::Failed | JobStatus::Cancelled)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Progress {
    /// Current playlist item (1-based) and total, when yt-dlp reports them.
    pub item: Option<u32>,
    pub total: Option<u32>,
    /// Download percent of the current item.
    pub percent: Option<f32>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Job {
    pub id: u64,
    pub url: String,
    pub mode: Mode,
    pub status: JobStatus,
    /// Playlist title, or "Artist - Title" for single tracks, once known.
    pub title: Option<String>,
    pub progress: Progress,
    pub tracks_done: u32,
    /// Most recent log line, for compact list views.
    pub last_line: Option<String>,
    /// Result summary or error text once finished.
    pub message: Option<String>,
    /// Unix milliseconds.
    pub created_at: u64,
    pub started_at: Option<u64>,
    pub finished_at: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LogLine {
    pub seq: u64,
    pub ts: u64,
    pub line: String,
}

/// Response for `GET /jobs/{id}/log?since=N`.
#[derive(Debug, Serialize)]
pub struct LogPage {
    pub lines: Vec<LogLine>,
    /// Pass this as `since` next time.
    pub next: u64,
    /// The job is finished; no more lines will arrive.
    pub done: bool,
    /// Some lines after `since` were already dropped from the ring buffer.
    pub truncated: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub enum CancelError {
    NotFound,
    AlreadyFinished(JobStatus),
}

/// A job handed to the worker.
pub struct Claim {
    pub id: u64,
    pub url: String,
    pub mode: Mode,
    pub cancel: Arc<AtomicBool>,
}

#[derive(Serialize, Deserialize)]
struct Entry {
    job: Job,
    log: VecDeque<LogLine>,
    next_seq: u64,
    #[serde(skip)]
    cancel: Arc<AtomicBool>,
}

impl Entry {
    fn push_line(&mut self, line: String) {
        self.job.last_line = Some(line.clone());
        self.log.push_back(LogLine { seq: self.next_seq, ts: now_ms(), line });
        self.next_seq += 1;
        while self.log.len() > MAX_LOG_LINES {
            self.log.pop_front();
        }
    }
}

#[derive(Default)]
struct Inner {
    entries: BTreeMap<u64, Entry>,
    queue: VecDeque<u64>,
    next_id: u64,
}

#[derive(Serialize, Deserialize)]
struct Persisted {
    version: u32,
    next_id: u64,
    entries: Vec<Entry>,
}

pub struct JobStore {
    inner: Mutex<Inner>,
    notify: Notify,
    persist_path: Option<PathBuf>,
}

pub fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// `[download]  45.3% of 3.45MiB at ...` → `Some(45.3)`.
pub fn parse_percent(line: &str) -> Option<f32> {
    let rest = line.strip_prefix("[download]")?.trim_start();
    let (num, _) = rest.split_once('%')?;
    num.trim().parse().ok()
}

/// `[download] Downloading item 3 of 12` → `Some((3, 12))`.
pub fn parse_item(line: &str) -> Option<(u32, u32)> {
    let rest = line.strip_prefix("[download] Downloading ")?;
    let rest = rest.strip_prefix("item ").or_else(|| rest.strip_prefix("video "))?;
    let (a, b) = rest.split_once(" of ")?;
    Some((a.trim().parse().ok()?, b.trim().parse().ok()?))
}

impl JobStore {
    /// An in-memory store (tests).
    pub fn in_memory() -> Self {
        Self { inner: Mutex::new(Inner { next_id: 1, ..Default::default() }), notify: Notify::new(), persist_path: None }
    }

    /// Loads history from `path` (if present). Jobs that were running when the
    /// process stopped are marked failed; queued jobs are queued again.
    pub fn load(path: PathBuf) -> Self {
        let mut inner = Inner { next_id: 1, ..Default::default() };
        match std::fs::read(&path) {
            Ok(bytes) => match serde_json::from_slice::<Persisted>(&bytes) {
                Ok(p) if p.version == PERSIST_VERSION => {
                    inner.next_id = p.next_id.max(1);
                    for mut e in p.entries {
                        match e.job.status {
                            JobStatus::Running => {
                                e.push_line("⚠️ Server restarted while this job was running.".into());
                                e.job.status = JobStatus::Failed;
                                e.job.message = Some("Interrupted by a server restart.".into());
                                e.job.finished_at = Some(now_ms());
                            }
                            JobStatus::Queued => inner.queue.push_back(e.job.id),
                            _ => {}
                        }
                        inner.next_id = inner.next_id.max(e.job.id + 1);
                        inner.entries.insert(e.job.id, e);
                    }
                    info!("Loaded {} job(s) from {:?}", inner.entries.len(), path);
                }
                Ok(p) => warn!("Ignoring {:?}: unknown version {}", path, p.version),
                Err(e) => {
                    // Keep the unreadable file for inspection rather than overwriting it.
                    let backup = path.with_extension(format!("json.corrupt-{}", now_ms()));
                    error!("Unreadable job history {:?} ({}); moved to {:?}", path, e, backup);
                    let _ = std::fs::rename(&path, &backup);
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => error!("Cannot read job history {:?}: {}", path, e),
        }
        let store = Self { inner: Mutex::new(inner), notify: Notify::new(), persist_path: Some(path) };
        store.persist();
        store
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Writes history atomically (temp file + rename). Errors are logged, not fatal.
    fn persist(&self) {
        let Some(path) = &self.persist_path else { return };
        let json = {
            let inner = self.lock();
            let snapshot = PersistedRef {
                version: PERSIST_VERSION,
                next_id: inner.next_id,
                entries: inner.entries.values().collect(),
            };
            serde_json::to_vec(&snapshot)
        };
        let result = json.map_err(|e| e.to_string()).and_then(|bytes| {
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
            }
            let tmp = path.with_extension("json.tmp");
            std::fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
            std::fs::rename(&tmp, path).map_err(|e| e.to_string())
        });
        if let Err(e) = result {
            error!("Failed to persist job history to {:?}: {}", path, e);
        }
    }

    /// Queues a job and wakes the worker.
    pub fn submit(&self, url: String, mode: Mode) -> Job {
        let job = {
            let mut inner = self.lock();
            let id = inner.next_id;
            inner.next_id += 1;
            let job = Job {
                id,
                url,
                mode,
                status: JobStatus::Queued,
                title: None,
                progress: Progress::default(),
                tracks_done: 0,
                last_line: None,
                message: None,
                created_at: now_ms(),
                started_at: None,
                finished_at: None,
            };
            inner.entries.insert(
                id,
                Entry { job: job.clone(), log: VecDeque::new(), next_seq: 1, cancel: Arc::default() },
            );
            inner.queue.push_back(id);
            job
        };
        info!("Queued job {} for {}", job.id, job.url);
        self.persist();
        self.notify.notify_one();
        job
    }

    /// Takes the next queued job and marks it running.
    pub fn claim_next(&self) -> Option<Claim> {
        let claim = {
            let mut inner = self.lock();
            let id = inner.queue.pop_front()?;
            let e = inner.entries.get_mut(&id)?;
            e.job.status = JobStatus::Running;
            e.job.started_at = Some(now_ms());
            Claim { id, url: e.job.url.clone(), mode: e.job.mode, cancel: e.cancel.clone() }
        };
        self.persist();
        Some(claim)
    }

    /// Resolves when a job may have been queued since the last call.
    pub async fn wait_for_work(&self) {
        self.notify.notified().await
    }

    /// Records one Engine output line. Percent-only progress lines update
    /// `progress` but are not stored (they would flood the log), except 100%.
    pub fn append_line(&self, id: u64, line: String) {
        let mut inner = self.lock();
        let Some(e) = inner.entries.get_mut(&id) else { return };
        if let Some(pct) = parse_percent(&line) {
            e.job.progress.percent = Some(pct);
            if pct < 100.0 {
                e.job.last_line = Some(line);
                return;
            }
        }
        if let Some((item, total)) = parse_item(&line) {
            e.job.progress.item = Some(item);
            e.job.progress.total = Some(total);
            e.job.progress.percent = Some(0.0);
        }
        e.push_line(line);
    }

    /// Records a finished track and sets the job title if it isn't known yet.
    pub fn record_track(&self, id: u64, track: &TrackMetadata, playlist_title: &str) {
        let mut inner = self.lock();
        let Some(e) = inner.entries.get_mut(&id) else { return };
        e.job.tracks_done += 1;
        if e.job.title.is_none() {
            e.job.title = Some(if playlist_title.is_empty() || playlist_title == "NA" {
                format!("{} - {}", track.artist, track.title)
            } else {
                playlist_title.to_string()
            });
        }
        e.push_line(format!("✓ Saved: {} - {}", track.artist, track.title));
    }

    /// Marks a job finished and prunes old history.
    pub fn finish(&self, id: u64, status: JobStatus, message: String) {
        debug_assert!(status.is_finished());
        {
            let mut inner = self.lock();
            let Some(e) = inner.entries.get_mut(&id) else { return };
            e.job.status = status;
            e.job.finished_at = Some(now_ms());
            e.push_line(match status {
                JobStatus::Succeeded => format!("✅ {}", message),
                JobStatus::Cancelled => format!("⏹ {}", message),
                _ => format!("❌ {}", message),
            });
            e.job.message = Some(message);
            prune(&mut inner);
        }
        info!("Job {} finished: {:?}", id, status);
        self.persist();
    }

    /// Cancels a queued job immediately, or signals a running job's Engine to stop.
    /// Returns the job's status after the call.
    pub fn cancel(&self, id: u64) -> Result<JobStatus, CancelError> {
        let status = {
            let mut inner = self.lock();
            let e = inner.entries.get_mut(&id).ok_or(CancelError::NotFound)?;
            match e.job.status {
                JobStatus::Queued => {
                    e.job.status = JobStatus::Cancelled;
                    e.job.finished_at = Some(now_ms());
                    e.job.message = Some("Cancelled before it started.".into());
                    e.push_line("⏹ Cancelled before it started.".into());
                    inner.queue.retain(|q| *q != id);
                    prune(&mut inner);
                    JobStatus::Cancelled
                }
                JobStatus::Running => {
                    e.cancel.store(true, Ordering::Release);
                    e.push_line("⏹ Cancel requested…".into());
                    JobStatus::Running
                }
                s => return Err(CancelError::AlreadyFinished(s)),
            }
        };
        self.persist();
        Ok(status)
    }

    pub fn get(&self, id: u64) -> Option<Job> {
        self.lock().entries.get(&id).map(|e| e.job.clone())
    }

    /// Clears all finished jobs and cancels all pending queued jobs. Leaves running job alone.
    /// Returns the number of jobs removed.
    pub fn clear(&self) -> usize {
        let mut inner = self.lock();
        inner.queue.clear();
        let before = inner.entries.len();
        inner.entries.retain(|_, e| e.job.status == JobStatus::Running);
        let removed = before - inner.entries.len();
        drop(inner);
        self.persist();
        removed
    }

    /// All jobs, newest first.
    pub fn list(&self) -> Vec<Job> {
        self.lock().entries.values().rev().map(|e| e.job.clone()).collect()
    }

    pub fn counts(&self) -> (usize, usize) {
        let inner = self.lock();
        let running = inner.entries.values().filter(|e| e.job.status == JobStatus::Running).count();
        (inner.queue.len(), running)
    }

    /// Log lines with `seq > since`.
    pub fn log_since(&self, id: u64, since: u64) -> Option<LogPage> {
        let inner = self.lock();
        let e = inner.entries.get(&id)?;
        let first_kept = e.log.front().map_or(e.next_seq, |l| l.seq);
        let lines: Vec<LogLine> = e.log.iter().filter(|l| l.seq > since).cloned().collect();
        Some(LogPage {
            next: lines.last().map_or(since.max(first_kept.saturating_sub(1)), |l| l.seq),
            truncated: since + 1 < first_kept,
            done: e.job.status.is_finished(),
            lines,
        })
    }
}

/// Serialization view that borrows entries instead of cloning every log.
#[derive(Serialize)]
struct PersistedRef<'a> {
    version: u32,
    next_id: u64,
    entries: Vec<&'a Entry>,
}

/// Drops the oldest finished jobs beyond [`MAX_HISTORY`].
fn prune(inner: &mut Inner) {
    let finished: Vec<u64> = inner
        .entries
        .values()
        .filter(|e| e.job.status.is_finished())
        .map(|e| e.job.id)
        .collect();
    if finished.len() > MAX_HISTORY {
        for id in &finished[..finished.len() - MAX_HISTORY] {
            inner.entries.remove(id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(t: &str) -> TrackMetadata {
        TrackMetadata { filepath: format!("/m/{t}.m4a"), title: t.into(), artist: "A".into(), duration: 1 }
    }

    fn temp_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "aes-jobs-{}-{}-{}/jobs.json",
            name,
            std::process::id(),
            now_ms()
        ))
    }

    #[test]
    fn parsers() {
        assert_eq!(parse_percent("[download]  45.3% of 3.45MiB at 1MiB/s"), Some(45.3));
        assert_eq!(parse_percent("[download] 100% of 3.45MiB"), Some(100.0));
        assert_eq!(parse_percent("[download] Destination: x.webm"), None);
        assert_eq!(parse_percent("[ExtractAudio] 50%"), None);
        assert_eq!(parse_item("[download] Downloading item 3 of 12"), Some((3, 12)));
        assert_eq!(parse_item("[download] Downloading video 1 of 2"), Some((1, 2)));
        assert_eq!(parse_item("[download] Downloading playlist: x"), None);
    }

    #[test]
    fn queue_is_fifo() {
        let s = JobStore::in_memory();
        let a = s.submit("u1".into(), Mode::Playlist);
        let b = s.submit("u2".into(), Mode::Song);
        assert!(b.id > a.id);
        assert_eq!(s.claim_next().unwrap().id, a.id);
        let c = s.claim_next().unwrap();
        assert_eq!((c.id, c.mode), (b.id, Mode::Song));
        assert!(s.claim_next().is_none());
        assert_eq!(s.get(a.id).unwrap().status, JobStatus::Running);
        assert_eq!(s.list()[0].id, b.id, "newest first");
    }

    #[test]
    fn log_cursor_returns_only_new_lines() {
        let s = JobStore::in_memory();
        let id = s.submit("u".into(), Mode::Playlist).id;
        s.append_line(id, "one".into());
        s.append_line(id, "two".into());
        let p = s.log_since(id, 0).unwrap();
        assert_eq!(p.lines.iter().map(|l| l.line.as_str()).collect::<Vec<_>>(), ["one", "two"]);
        assert_eq!(p.next, 2);
        assert!(!p.done && !p.truncated);

        s.append_line(id, "three".into());
        let p = s.log_since(id, 2).unwrap();
        assert_eq!(p.lines.len(), 1);
        assert_eq!(p.lines[0].line, "three");
        assert_eq!(p.next, 3);

        let p = s.log_since(id, 3).unwrap();
        assert!(p.lines.is_empty());
        assert_eq!(p.next, 3, "cursor is stable when nothing is new");
    }

    #[test]
    fn progress_lines_update_progress_but_not_log() {
        let s = JobStore::in_memory();
        let id = s.submit("u".into(), Mode::Playlist).id;
        s.append_line(id, "[download] Downloading item 2 of 5".into());
        s.append_line(id, "[download]  12.0% of 3MiB".into());
        s.append_line(id, "[download]  50.5% of 3MiB".into());
        let job = s.get(id).unwrap();
        assert_eq!(job.progress, Progress { item: Some(2), total: Some(5), percent: Some(50.5) });
        assert_eq!(job.last_line.as_deref(), Some("[download]  50.5% of 3MiB"));
        s.append_line(id, "[download] 100% of 3MiB".into());
        let lines: Vec<_> = s.log_since(id, 0).unwrap().lines.into_iter().map(|l| l.line).collect();
        assert_eq!(lines, ["[download] Downloading item 2 of 5", "[download] 100% of 3MiB"]);
    }

    #[test]
    fn ring_buffer_truncates_and_reports_it() {
        let s = JobStore::in_memory();
        let id = s.submit("u".into(), Mode::Playlist).id;
        for i in 0..(MAX_LOG_LINES + 10) {
            s.append_line(id, format!("l{i}"));
        }
        let p = s.log_since(id, 0).unwrap();
        assert_eq!(p.lines.len(), MAX_LOG_LINES);
        assert!(p.truncated);
        assert_eq!(p.lines[0].seq, 11);
        assert_eq!(p.next, (MAX_LOG_LINES + 10) as u64);
        // A client that is caught up is not told it missed anything.
        assert!(!s.log_since(id, p.next).unwrap().truncated);
    }

    #[test]
    fn tracks_set_title_and_count() {
        let s = JobStore::in_memory();
        let single = s.submit("u".into(), Mode::Song).id;
        s.record_track(single, &track("Song"), "NA");
        assert_eq!(s.get(single).unwrap().title.as_deref(), Some("A - Song"));

        let pl = s.submit("u".into(), Mode::Playlist).id;
        s.record_track(pl, &track("x"), "Mix");
        s.record_track(pl, &track("y"), "Mix");
        let job = s.get(pl).unwrap();
        assert_eq!((job.title.as_deref(), job.tracks_done), (Some("Mix"), 2));
    }

    #[test]
    fn cancel_queued_running_and_finished() {
        let s = JobStore::in_memory();
        let running = s.submit("a".into(), Mode::Playlist).id;
        let queued = s.submit("b".into(), Mode::Playlist).id;
        let claim = s.claim_next().unwrap();
        assert_eq!(claim.id, running);

        assert_eq!(s.cancel(queued), Ok(JobStatus::Cancelled));
        assert!(s.claim_next().is_none(), "cancelled job left the queue");

        assert_eq!(s.cancel(running), Ok(JobStatus::Running));
        assert!(claim.cancel.load(Ordering::Acquire), "worker sees the flag");

        s.finish(running, JobStatus::Cancelled, "Cancelled.".into());
        assert_eq!(s.cancel(running), Err(CancelError::AlreadyFinished(JobStatus::Cancelled)));
        assert_eq!(s.cancel(999), Err(CancelError::NotFound));
        assert!(s.log_since(running, 0).unwrap().done);
    }

    #[test]
    fn history_is_pruned() {
        let s = JobStore::in_memory();
        for _ in 0..(MAX_HISTORY + 5) {
            let id = s.submit("u".into(), Mode::Playlist).id;
            s.claim_next();
            s.finish(id, JobStatus::Succeeded, "ok".into());
        }
        let jobs = s.list();
        assert_eq!(jobs.len(), MAX_HISTORY);
        assert_eq!(jobs.last().unwrap().id, 6, "oldest were dropped");
    }

    #[test]
    fn persistence_round_trip_and_restart_recovery() {
        let path = temp_path("persist");
        {
            let s = JobStore::load(path.clone());
            let done = s.submit("done".into(), Mode::Playlist).id;
            s.claim_next();
            s.append_line(done, "hello".into());
            s.finish(done, JobStatus::Succeeded, "ok".into());
            let _running = s.submit("running".into(), Mode::Playlist).id;
            s.claim_next();
            let _queued = s.submit("queued".into(), Mode::Song).id;
        }
        let s = JobStore::load(path.clone());
        let jobs = s.list();
        assert_eq!(jobs.len(), 3);
        let by_url = |u: &str| jobs.iter().find(|j| j.url == u).unwrap().clone();

        assert_eq!(by_url("done").status, JobStatus::Succeeded);
        let log = s.log_since(by_url("done").id, 0).unwrap();
        assert_eq!(log.lines[0].line, "hello");

        let r = by_url("running");
        assert_eq!(r.status, JobStatus::Failed);
        assert!(r.message.unwrap().contains("restart"));

        assert_eq!(by_url("queued").status, JobStatus::Queued);
        assert_eq!(s.claim_next().unwrap().url, "queued", "queued jobs survive restarts");
        assert!(s.submit("new".into(), Mode::Playlist).id > by_url("queued").id, "ids are not reused");

        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn clear_removes_finished_and_queued() {
        let s = JobStore::in_memory();
        let running = s.submit("r".into(), Mode::Song).id;
        s.claim_next(); // claims running
        let queued = s.submit("q".into(), Mode::Song).id; // queued
        let _finished = s.submit("f".into(), Mode::Song).id; // queued
        let c = s.claim_next().unwrap(); // claims queued
        assert_eq!(c.id, queued);
        s.finish(queued, JobStatus::Succeeded, "ok".into());

        // Now: running is Running, queued is Succeeded (finished), finished is Queued
        assert_eq!(s.clear(), 2); // removes finished (Queued) and queued (Succeeded)
        assert_eq!(s.list().len(), 1);
        assert_eq!(s.list()[0].id, running);
        assert!(s.claim_next().is_none(), "queue is empty");
    }

    #[test]
    fn corrupt_history_is_ignored() {
        let path = temp_path("corrupt");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "{not json").unwrap();
        let s = JobStore::load(path.clone());
        assert!(s.list().is_empty());
        assert_eq!(s.submit("u".into(), Mode::Playlist).id, 1);
        let backups = std::fs::read_dir(path.parent().unwrap())
            .unwrap()
            .filter(|e| e.as_ref().unwrap().file_name().to_string_lossy().contains("corrupt"))
            .count();
        assert_eq!(backups, 1, "corrupt file is kept, not overwritten");
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
}
