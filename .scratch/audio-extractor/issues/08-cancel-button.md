# 08 — Cancel Button

**What to build:** While an extraction is actively running, the user should have the option to click a "Cancel" button. Clicking this button safely terminates the underlying `yt-dlp` process and resets the UI, ensuring no lingering background downloads.

**Blocked by:** 04 — Global Progress Bar Streaming

**Status:** ready-for-agent

- [ ] The UI dynamically displays a "Cancel" button while an extraction is in progress.
- [ ] Clicking the "Cancel" button invokes a new Tauri command in the Rust backend.
- [ ] The Rust backend successfully sends a termination signal to the running `yt-dlp` subprocess.
- [ ] The UI gracefully resets to the default state and displays a "Download Canceled" message.
