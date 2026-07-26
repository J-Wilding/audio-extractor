# 04 — Progress Streaming

**What to build:** The user sees real-time, line-by-line `yt-dlp` logs streaming into the CRT UI as the download progresses, rather than staring at a silent spinner until the entire extraction finishes.

**Blocked by:** None — can start immediately.

**Status:** ready-for-agent

- [x] The Rust backend streams the subprocess stdout line-by-line via Tauri events.
- [x] The frontend listens to the event and dynamically updates the `#status-msg` box.
- [x] The `#status-msg` box automatically scrolls to the bottom as new logs arrive.
