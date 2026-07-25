# 02 — Engine Download, Update Lifecycle, and Lock

**What to build:** When the app starts, the Rust backend silently fetches the latest `yt-dlp` binary to the local app data folder and acquires the Engine Lock. If the user tries to click the button during this phase, the UI politely shows "Updating Engine..." until the background thread releases the lock.

**Blocked by:** 01 — Scaffold Tauri App and Core UI Layout

**Status:** ready-for-agent

- [ ] On startup, Rust spawns a background thread that checks for the `yt-dlp` binary in the app data directory.
- [ ] If missing, it downloads the standalone executable.
- [ ] If present, it executes `yt-dlp -U` to update it.
- [ ] An "Engine Lock" state is maintained in Rust and communicated to the frontend.
- [ ] The UI disables the "Extract Audio" button and shows an "Updating Engine..." message while the lock is held.
