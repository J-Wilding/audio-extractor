//! The extraction core: runs the Engine (yt-dlp) for one URL, streams its output
//! as [`Event`]s, and reports which tracks landed where. No HTTP or job concerns here.

use std::io::{BufReader, Read};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use log::{debug, error, info, warn};
use serde::{Deserialize, Serialize};

use crate::plex::{PlaylistBatch, TrackMetadata};

pub const TRACK_DELIM_PREFIX: &str = "YTDLM_TRACK:::";

/// Output template under the music root (Metadata Routing, see CONTEXT.md).
pub const OUTPUT_TEMPLATE: &str =
    "%(artist,uploader|Unknown Artist)s/%(album,playlist|Singles)s/%(playlist_index,track_number&{:02d} - |)s%(title)s.%(ext)s";

/// How long a cancelled Engine gets to exit after SIGTERM before SIGKILL.
const CANCEL_GRACE: Duration = Duration::from_secs(3);

/// What the user asked for. `Song` forces a single video even if the URL has `list=`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Song,
    #[default]
    Playlist,
}

pub struct ExtractionRequest<'a> {
    pub engine_path: &'a Path,
    pub music_root: &'a Path,
    pub temp_dir: &'a Path,
    pub url: &'a str,
    pub mode: Mode,
    /// Set to `true` from another thread to stop the Engine.
    pub cancel: &'a AtomicBool,
}

#[derive(Debug)]
pub enum Event {
    /// One line of Engine stdout/stderr.
    Line(String),
    /// A track finished and was moved into the music root.
    Track { track: TrackMetadata, playlist_title: String },
}

#[derive(Debug, Default)]
pub struct Outcome {
    pub tracks: Vec<TrackMetadata>,
    /// Set only for non-album playlists, which get a Playlist File.
    pub playlist: Option<PlaylistBatch>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ExtractError {
    Cancelled,
    Failed(String),
}

impl std::fmt::Display for ExtractError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExtractError::Cancelled => f.write_str("Cancelled."),
            ExtractError::Failed(m) => f.write_str(m),
        }
    }
}

/// Reads `reader` to EOF, calling `emit` for every non-empty line. Both `\n` and
/// `\r` end a line, so yt-dlp's in-place progress updates arrive individually.
fn stream_lines<R: Read>(reader: R, mut emit: impl FnMut(String)) {
    let mut buf = Vec::new();
    for byte in BufReader::new(reader).bytes() {
        let Ok(c) = byte else { break };
        if c == b'\n' || c == b'\r' {
            if !buf.is_empty() {
                emit(String::from_utf8_lossy(&buf).into_owned());
                buf.clear();
            }
        } else {
            buf.push(c);
        }
    }
    if !buf.is_empty() {
        emit(String::from_utf8_lossy(&buf).into_owned());
    }
}

/// Track info printed via `--print after_move:...`.
#[derive(Debug, PartialEq)]
pub struct TrackLine {
    pub track: TrackMetadata,
    pub playlist_title: String,
    pub playlist_id: String,
    pub album: String,
}

pub fn parse_track_delim_line(line: &str) -> Option<TrackLine> {
    let parts: Vec<&str> = line.strip_prefix(TRACK_DELIM_PREFIX)?.split(":::").collect();
    if parts.len() < 7 {
        return None;
    }
    Some(TrackLine {
        track: TrackMetadata {
            filepath: parts[0].to_string(),
            title: parts[1].to_string(),
            artist: parts[2].to_string(),
            duration: parts[3].parse::<f64>().map(|d| d.round() as u64).unwrap_or(0),
        },
        playlist_title: parts[4].to_string(),
        playlist_id: parts[5].to_string(),
        album: parts[6].to_string(),
    })
}

/// Whether a playlist should get a Playlist File. YouTube Music albums
/// (`OLAK5uy_…`) already appear as albums in Plex, so they don't.
pub fn is_custom_playlist(playlist_id: &str, playlist_title: &str, album: &str) -> bool {
    let missing = |s: &str| s.is_empty() || s == "NA";
    if missing(playlist_id) || missing(playlist_title) || playlist_id.starts_with("OLAK5uy_") {
        return false;
    }
    !(!missing(album) && album.eq_ignore_ascii_case(playlist_title))
}

fn signal_group(pid: u32, sig: libc::c_int) {
    // SAFETY: plain syscall; a negative pid targets the process group we created
    // with `process_group(0)`, so ffmpeg children are signalled too.
    unsafe {
        libc::kill(-(pid as libc::pid_t), sig);
    }
}

const SKIP_EXISTING_META_PLUGIN: &str = r#"import os
from yt_dlp.postprocessor.ffmpeg import FFmpegMetadataPP

_orig_run = FFmpegMetadataPP.run

def _patched_run(self, info):
    filepath = os.path.abspath(info.get("filepath") or "")
    home_dir = os.path.abspath((self._downloader.params.get("paths") or {}).get("home") or "")
    if home_dir and (filepath == home_dir or filepath.startswith(home_dir + os.sep)):
        self.to_screen(f"Already in library, skipping metadata rewrite: {filepath}")
        return [], info
    return _orig_run(self, info)

FFmpegMetadataPP.run = _patched_run
"#;

/// Installs a tiny yt-dlp plugin in `temp_dir` that skips re-running `ffmpeg`
/// metadata embedding on tracks that already exist in `home:` (`music_root`).
/// This avoids redundant `ffmpeg` passes and macOS SMB `Errno 83` in-place renames.
fn ensure_plugin_dir(temp_dir: &Path) -> Option< std::path::PathBuf > {
    let root = temp_dir.join("plugins");
    let pkg = root.join("aes/yt_dlp_plugins/postprocessor");
    std::fs::create_dir_all(&pkg).ok()?;
    std::fs::write(pkg.join("skip_existing_meta.py"), SKIP_EXISTING_META_PLUGIN).ok()?;
    Some(root)
}

/// Runs the Engine against `req.url`, blocking until it exits. Callers serialize
/// jobs via the Engine Lock.
pub fn extract<F>(req: &ExtractionRequest, on_event: F) -> Result<Outcome, ExtractError>
where
    F: Fn(Event) + Sync,
{
    if !req.music_root.is_dir() {
        warn!("Music root {:?} is missing", req.music_root);
        return Err(ExtractError::Failed(format!(
            "Music folder {} is not available. Check the volume mount.",
            req.music_root.display()
        )));
    }
    if !req.engine_path.exists() {
        error!("Engine binary missing at {:?}", req.engine_path);
        return Err(ExtractError::Failed(
            "Engine not found. It may still be downloading; try again shortly.".into(),
        ));
    }
    let _ = std::fs::create_dir_all(req.temp_dir);

    info!("Starting extraction for {} ({:?})", req.url, req.mode);
    let print_arg = format!(
        "after_move:{}%(filepath)s:::%(title)s:::%(artist,uploader|Unknown Artist)s:::%(duration)s:::%(playlist_title)s:::%(playlist_id)s:::%(album)s",
        TRACK_DELIM_PREFIX
    );
    // Homebrew paths let local macOS dev runs find ffmpeg; harmless in the container.
    let path = format!(
        "{}:/opt/homebrew/bin:/usr/local/bin",
        std::env::var("PATH").unwrap_or_default()
    );

    let mut cmd = Command::new(req.engine_path);
    cmd.env("PATH", path);
    if let Some(plugin_dir) = ensure_plugin_dir(req.temp_dir) {
        cmd.arg("--plugin-dirs").arg(plugin_dir);
    }
    cmd.args([
        "--no-quiet",
        "--newline",
        "--ignore-errors",
        "-x",
        "--audio-format",
        "m4a",
        "--embed-metadata",
    ])
    .arg("--paths")
    .arg(format!("home:{}", req.music_root.display()))
    .arg("--paths")
    .arg(format!("temp:{}", req.temp_dir.display()))
    .args(["-o", OUTPUT_TEMPLATE, "--print", &print_arg]);
    if req.mode == Mode::Song {
        cmd.arg("--no-playlist");
    }
    debug!("Engine command: {:?}", cmd);
    let mut child = cmd
        .arg(req.url)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .spawn()
        .map_err(|e| ExtractError::Failed(format!("Failed to start the Engine: {}", e)))?;

    let pid = child.id();
    let stdout = child.stdout.take().expect("stdout is piped");
    let stderr = child.stderr.take().expect("stderr is piped");

    let tracks: Mutex<Vec<TrackMetadata>> = Mutex::default();
    // (playlist_title, playlist_id, album) of the first track that had one.
    let playlist_meta: Mutex<Option<(String, String, String)>> = Mutex::default();
    let readers_done = AtomicBool::new(false);
    let on_event = &on_event;

    std::thread::scope(|s| {
        let out = s.spawn(|| {
            stream_lines(stdout, |line| match parse_track_delim_line(&line) {
                Some(t) => {
                    tracks.lock().unwrap().push(t.track.clone());
                    let mut meta = playlist_meta.lock().unwrap();
                    if meta.is_none() && t.playlist_title != "NA" {
                        *meta = Some((t.playlist_title.clone(), t.playlist_id, t.album));
                    }
                    drop(meta);
                    on_event(Event::Track { track: t.track, playlist_title: t.playlist_title });
                }
                None => on_event(Event::Line(line)),
            })
        });
        let err = s.spawn(|| stream_lines(stderr, |line| on_event(Event::Line(line))));

        // Cancellation watcher: SIGTERM the whole group, then SIGKILL after a grace period.
        s.spawn(|| {
            let mut term_sent: Option<Instant> = None;
            while !readers_done.load(Ordering::Acquire) {
                if req.cancel.load(Ordering::Acquire) {
                    match term_sent {
                        None => {
                            info!("Cancelling Engine process group {}", pid);
                            signal_group(pid, libc::SIGTERM);
                            term_sent = Some(Instant::now());
                        }
                        Some(t) if t.elapsed() > CANCEL_GRACE => {
                            warn!("Engine ignored SIGTERM; sending SIGKILL");
                            signal_group(pid, libc::SIGKILL);
                            break;
                        }
                        _ => {}
                    }
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        });

        let _ = out.join();
        let _ = err.join();
        readers_done.store(true, Ordering::Release);
    });

    let status = child
        .wait()
        .map_err(|e| ExtractError::Failed(format!("Failed to wait on the Engine: {}", e)))?;

    if req.cancel.load(Ordering::Acquire) {
        info!("Extraction cancelled for {}", req.url);
        return Err(ExtractError::Cancelled);
    }

    let tracks = tracks.into_inner().unwrap();
    if !status.success() {
        if tracks.is_empty() {
            error!("Extraction failed: {}", status);
            return Err(ExtractError::Failed(format!("Extraction failed ({}).", status)));
        }
        warn!(
            "Engine exited with {} after saving {} track(s); continuing with partial playlist.",
            status,
            tracks.len()
        );
        on_event(Event::Line(format!(
            "⚠️ Some items in the playlist had errors ({}), continuing with {} saved track(s).",
            status,
            tracks.len()
        )));
    }
    let playlist = match playlist_meta.into_inner().unwrap() {
        Some((title, id, album)) if is_custom_playlist(&id, &title, &album) && !tracks.is_empty() => {
            Some(PlaylistBatch { playlist_title: title, tracks: tracks.clone() })
        }
        _ => None,
    };
    info!("Extraction finished: {} track(s)", tracks.len());
    Ok(Outcome { tracks, playlist })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;

    struct Fixture {
        dir: PathBuf,
        music: PathBuf,
        engine: PathBuf,
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.dir);
        }
    }

    fn fixture(name: &str, script_body: &str) -> Fixture {
        let dir = std::env::temp_dir().join(format!(
            "aes-extract-{}-{}-{}",
            name,
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        let music = dir.join("music");
        fs::create_dir_all(&music).unwrap();
        let engine = dir.join("fake-yt-dlp");
        fs::write(&engine, format!("#!/bin/sh\n{}\n", script_body)).unwrap();
        fs::set_permissions(&engine, fs::Permissions::from_mode(0o755)).unwrap();
        Fixture { dir, music, engine }
    }

    fn run_with(
        f: &Fixture,
        engine: &Path,
        music: &Path,
        mode: Mode,
        cancel: &AtomicBool,
    ) -> (Result<Outcome, ExtractError>, Vec<String>) {
        let lines = Mutex::new(Vec::new());
        let temp = f.dir.join("tmp");
        let req = ExtractionRequest {
            engine_path: engine,
            music_root: music,
            temp_dir: &temp,
            url: "https://youtu.be/x",
            mode,
            cancel,
        };
        let result = extract(&req, |e| {
            if let Event::Line(l) = e {
                lines.lock().unwrap().push(l);
            }
        });
        (result, lines.into_inner().unwrap())
    }

    fn run(f: &Fixture) -> (Result<Outcome, ExtractError>, Vec<String>) {
        run_with(f, &f.engine, &f.music, Mode::Playlist, &AtomicBool::new(false))
    }

    #[test]
    fn test_parse_track_delim_line() {
        let t = parse_track_delim_line(
            "YTDLM_TRACK:::/path/to/song.m4a:::My Title:::My Artist:::180.4:::My Playlist:::PL123:::NA",
        )
        .unwrap();
        assert_eq!(t.track.filepath, "/path/to/song.m4a");
        assert_eq!(t.track.title, "My Title");
        assert_eq!(t.track.artist, "My Artist");
        assert_eq!(t.track.duration, 180);
        assert_eq!(t.playlist_title, "My Playlist");
        assert_eq!(t.playlist_id, "PL123");
        assert_eq!(t.album, "NA");
        assert!(parse_track_delim_line("not a track").is_none());
    }

    #[test]
    fn test_is_custom_playlist() {
        assert!(!is_custom_playlist("OLAK5uy_abc123", "Some Album", "Some Album"));
        assert!(!is_custom_playlist("NA", "NA", "NA"));
        assert!(!is_custom_playlist("PL1", "Same", "same"));
        assert!(is_custom_playlist("PL_12345", "Summer Vibes", "NA"));
        assert!(is_custom_playlist("PL_12345", "Summer Vibes", "Various"));
    }

    #[test]
    fn stream_lines_splits_on_cr_and_lf() {
        let mut out = Vec::new();
        stream_lines(&b"a\r\nb\rc\n\nd"[..], |l| out.push(l));
        assert_eq!(out, vec!["a", "b", "c", "d"]);
    }

    #[test]
    fn success_streams_output_and_passes_url_last() {
        let f = fixture(
            "ok",
            r#"for last; do :; done
printf 'url=%s\n' "$last"
printf '[download]  10%%\r[download] 100%%\n'
echo 'WARNING: something' 1>&2
exit 0"#,
        );
        let (result, lines) = run(&f);
        let outcome = result.unwrap();
        assert!(outcome.tracks.is_empty());
        assert!(outcome.playlist.is_none());
        assert!(lines.contains(&"url=https://youtu.be/x".to_string()));
        assert!(lines.contains(&"[download] 100%".to_string()));
        assert!(lines.contains(&"WARNING: something".to_string()));
    }

    #[test]
    fn song_mode_adds_no_playlist() {
        let f = fixture("song", r#"for a; do echo "arg=$a"; done"#);
        let (_, lines) = run_with(&f, &f.engine, &f.music, Mode::Song, &AtomicBool::new(false));
        assert!(lines.contains(&"arg=--no-playlist".to_string()));
        let (_, lines) = run(&f);
        assert!(!lines.contains(&"arg=--no-playlist".to_string()));
    }

    #[test]
    fn custom_playlist_tracks_produce_a_batch() {
        let f = fixture(
            "pl",
            "echo 'YTDLM_TRACK:::/m/A/Mix/01 - x.m4a:::x:::A:::10:::Mix:::PL9:::NA'\n\
             echo 'YTDLM_TRACK:::/m/B/Mix/02 - y.m4a:::y:::B:::20:::Mix:::PL9:::NA'",
        );
        let outcome = run(&f).0.unwrap();
        assert_eq!(outcome.tracks.len(), 2);
        let batch = outcome.playlist.unwrap();
        assert_eq!(batch.playlist_title, "Mix");
        assert_eq!(batch.tracks[1].duration, 20);
    }

    #[test]
    fn album_tracks_do_not_produce_a_batch() {
        let f = fixture("album", "echo 'YTDLM_TRACK:::/m/A/Al/01 - x.m4a:::x:::A:::10:::Al:::OLAK5uy_1:::Al'");
        let outcome = run(&f).0.unwrap();
        assert_eq!(outcome.tracks.len(), 1);
        assert!(outcome.playlist.is_none());
    }

    #[test]
    fn nonzero_exit_is_an_error_and_stderr_is_still_reported() {
        let f = fixture("fail", "echo 'ERROR: Video unavailable' 1>&2\nexit 1");
        let (result, lines) = run(&f);
        match result {
            Err(ExtractError::Failed(m)) => assert!(m.starts_with("Extraction failed"), "{m}"),
            other => panic!("unexpected {other:?}"),
        }
        assert_eq!(lines, vec!["ERROR: Video unavailable"]);
    }

    #[test]
    fn partial_playlist_success_keeps_saved_tracks() {
        let f = fixture(
            "partial",
            "echo 'YTDLM_TRACK:::/m/A/Mix/01 - x.m4a:::x:::A:::10:::Mix:::PL9:::NA'\n\
             echo 'ERROR: Item 2 unavailable' 1>&2\n\
             exit 1",
        );
        let (result, lines) = run(&f);
        let outcome = result.unwrap();
        assert_eq!(outcome.tracks.len(), 1);
        assert_eq!(outcome.playlist.unwrap().playlist_title, "Mix");
        assert!(lines.iter().any(|l| l.contains("Some items in the playlist had errors")));
    }

    #[test]
    fn missing_music_root_fails_before_running_engine() {
        let f = fixture("nomusic", "echo should-not-run");
        let missing = f.dir.join("not-mounted");
        let (result, lines) = run_with(&f, &f.engine, &missing, Mode::Playlist, &AtomicBool::new(false));
        assert!(matches!(result, Err(ExtractError::Failed(m)) if m.contains("not available")));
        assert!(lines.is_empty());
    }

    #[test]
    fn missing_engine_is_reported() {
        let f = fixture("noengine", "exit 0");
        let missing = f.dir.join("missing");
        let (result, _) = run_with(&f, &missing, &f.music, Mode::Playlist, &AtomicBool::new(false));
        assert!(matches!(result, Err(ExtractError::Failed(m)) if m.starts_with("Engine not found")));
    }

    #[test]
    fn cancel_kills_the_engine_and_its_children() {
        // `sleep` is a child of the shell, like ffmpeg is a child of yt-dlp.
        let f = fixture("cancel", "echo started\nsleep 30\necho should-not-print");
        let cancel = AtomicBool::new(false);
        let start = Instant::now();
        let (result, lines) = std::thread::scope(|s| {
            s.spawn(|| {
                std::thread::sleep(Duration::from_millis(300));
                cancel.store(true, Ordering::Release);
            });
            run_with(&f, &f.engine, &f.music, Mode::Playlist, &cancel)
        });
        assert_eq!(result.unwrap_err(), ExtractError::Cancelled);
        assert!(start.elapsed() < Duration::from_secs(5), "took {:?}", start.elapsed());
        assert!(!lines.contains(&"should-not-print".to_string()));
    }
}
