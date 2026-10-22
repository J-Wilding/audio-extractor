# Audio Extractor

> **Note (v2 Architecture):** The Tauri desktop app (`src-tauri/`) is **deprecated and frozen**. Audio Extractor now runs as a **headless backend service on the QNAP NAS (`server/`)** paired with the **Chromium Browser Extension (`extension/`)**, which includes a persistent Side Panel and Pop-out window for live job logs. See [`server/README.md`](server/README.md) for NAS deployment instructions.

---

## Architecture (v2)

1. **NAS Backend Service (`server/`)**:
   - Headless Rust HTTP service deployed via Docker on the QNAP NAS alongside Plex.
   - Manages the `yt-dlp` Engine (auto-downloads and updates every 24h), FIFO job queue, per-job ring-buffered logs (`GET /jobs/{id}/log?since=N`), extended `.m3u` generation, and Plex playlist import.
   - Writes directly to the NAS Music volume (`/music`), eliminating macOS SMB mount and Unicode NFD/NFC issues.

2. **Browser Extension (`extension/`)**:
   - **Toolbar Popup**: Quick 1-click submission for the active YouTube / YouTube Music tab (classifies Song, Album, Playlist, or prompts when a song is inside a playlist).
   - **Persistent Side Panel (`sidepanel.html`)**: Stays open across tabs in Chrome. Displays the job queue, history, progress bars, cancel buttons, and a live auto-scrolling log that catches up from the start of the job whenever opened.
   - **Pop-out Window (`sidepanel.html?popout=1`)**: Opens the Side Panel view in a standalone floating window (`440×680`).
   - **Toolbar Badge**: Background service worker polls `/health` and displays the count of active (`running + queued`) jobs on the extension icon.

---

## Loading the Browser Extension

1. Open Google Chrome (or Brave / Edge) and navigate to `chrome://extensions`.
2. Enable **Developer mode** (top right).
3. Click **Load unpacked** (or **Reload** if already loaded) and select the `extension/` folder.
4. Click the extension icon → **Settings ⚙️**:
   - Set **Server URL** (e.g. `http://10.0.0.69:47821` for the QNAP NAS, or `http://127.0.0.1:47821` for local testing).
   - Paste your `BRIDGE_TOKEN`.
   - Click **Test Connection**, then **Save Settings**.

---

## Running the Backend Server

### Option A: Docker on QNAP NAS (Recommended)
See [`server/README.md`](server/README.md) for full `docker-compose.yml` instructions.

### Option B: Directly with Cargo (Local Dev / Testing)
```bash
cd server
BRIDGE_TOKEN="$(openssl rand -hex 24)" \
MUSIC_ROOT="/Volumes/data/Media/Music" \
DATA_DIR="/tmp/audio-extractor-data" \
PLEX_URL="http://10.0.0.69:32400" \
cargo run
```
