# Audio Extractor Server (QNAP / Docker)

Headless Rust service that runs on the QNAP NAS alongside Plex. It accepts YouTube and YouTube Music links from the browser extension, extracts M4A audio with embedded metadata directly into the Music share, generates extended `.m3u` files for non-album playlists, and imports them into Plex.

## Quick Start on QNAP Container Station

1. **Generate a Bridge Token** on any Mac:
   ```bash
   openssl rand -hex 24
   ```

2. **Copy `.env.example` to `.env`** and fill in `BRIDGE_TOKEN` and `PLEX_TOKEN`:
   ```bash
   cp .env.example .env
   ```
   *(To find your `PLEX_TOKEN`, open any item in Plex Web → **Get Info** → **View XML** → copy `X-Plex-Token` from the URL bar.)*

3. **Deploy in Container Station** (or via SSH):
   - In QNAP **Container Station** → **Applications** → **Create**, paste the contents of `docker-compose.yml` (or run `docker compose up -d` via SSH).
   - Verify the service is healthy:
     ```bash
     curl http://10.0.0.69:47821/health
     ```

4. **Configure the Browser Extension** on each Mac:
   - Open extension **Settings ⚙️**
   - Set **Server URL** to `http://10.0.0.69:47821`
   - Paste your `BRIDGE_TOKEN`
   - Click **Test Connection** and **Save**

## Updating

The server automatically updates `yt-dlp` at startup and every 24 hours while idle. To update the server image itself:

```bash
docker compose pull && docker compose up -d
```

## Environment Variables

| Variable | Default | Description |
|---|---|---|
| `BRIDGE_TOKEN` | *(required)* | Shared secret (minimum 24 chars) sent in `X-Bridge-Token` |
| `PORT` | `47821` | HTTP listening port |
| `MUSIC_ROOT` | `/music` | Music directory inside the container |
| `DATA_DIR` | `/data` | Persistent directory for `yt-dlp` binary and `jobs.json` |
| `PLAYLISTS_DIR` | `/music/Playlists` | Directory where `.m3u` files are written |
| `PLEX_URL` | `http://10.0.0.69:32400` | Plex Media Server URL (auto-prefixes `http://` if omitted) |
| `PLEX_TOKEN` | *(optional)* | Plex token (`X-Plex-Token`). If omitted, Plex import is skipped |
| `PLEX_SECTION_ID` | `3` | Plex Music library section ID |
| `PLEX_MUSIC_ROOT` | `/share/CACHEDEV1_DATA/Multimedia/Media/Music` | Music root path as seen by the Plex server |
| `PUID` / `PGID` | `1000` / `1000` | UID and GID used to write files into the Music share |
