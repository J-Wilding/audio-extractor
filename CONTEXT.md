# Domain Glossary

- **Audio Extractor**: A hyper-focused tool that takes a YouTube or YouTube Music URL and extracts the highest quality audio (M4A with embedded metadata), hiding the complexity of `yt-dlp` from the user. It is explicitly not a general-purpose video downloader.
- **Backend Service**: The headless Rust HTTP server (`server/`) running in Docker on the QNAP NAS alongside Plex. It owns the Engine, the FIFO job queue, per-job logs, `.m3u` generation, and Plex import. Protected by a shared `BRIDGE_TOKEN` in the `X-Bridge-Token` header.
- **Engine**: The underlying `yt-dlp` executable that powers the downloads. It is treated as an ephemeral dependency, downloaded on first start into `DATA_DIR` and updated via `yt-dlp -U`.
- **Engine Lock**: A concurrency mechanism in the backend that prevents download tasks from executing while the Engine is being downloaded or updated, and serializes extraction jobs so one runs at a time.
- **NAS Root / Music Root**: The base directory where all audio is saved (`/music` inside the NAS container, mapped to `/share/CACHEDEV1_DATA/Multimedia/Media/Music`).
- **Metadata Routing**: The strategy of using `yt-dlp`'s output templates to automatically create Artist and Album subfolders inside the Music Root without user intervention.
- **Job Log**: A per-job ring buffer (up to 2,000 lines with monotonically increasing `seq` numbers) stored by the Backend Service and persisted to `DATA_DIR/jobs.json`. Clients fetch lines incrementally via `GET /jobs/{id}/log?since=<seq>`, allowing any client that opens mid-job to catch up from line 1 and continue receiving live updates.
- **Side Panel / Pop-out Window**: The persistent extension UI (`extension/sidepanel.html`) that stays open while browsing (docked in Chrome's Side Panel or floated as a popup window), displaying the job queue, history, cancel controls, and Live Job Log.
- **Playlist File**: An extended `.m3u` (with `#EXTM3U` / `#EXTINF` lines and absolute, Plex-server-relative paths) generated for non-album playlists and imported into Plex via `/playlists/upload`. Albums never get a Playlist File.
- **Browser Bridge (Legacy)**: The localhost-only HTTP server inside the deprecated Tauri desktop app (`src-tauri/`), superseded by the Backend Service.
