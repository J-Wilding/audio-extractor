# 12 — Generate Playlist File (.m3u) and Import into Plex for Non-Album Lists

**What to build:** When a job is a non-album playlist (not a YouTube Music album), after all tracks land on the NAS the app writes an extended `.m3u` with absolute paths and imports it into Plex via the API, so the playlist shows up in Plex automatically. Albums are not turned into playlists — Plex already groups them as albums.

**Blocked by:** 05 — Playlist Support, 07 — Plex Integration (shares Plex URL/token/section config)

**Status:** done

- [x] Album vs playlist detection: YouTube Music album lists (`OLAK5uy_` prefix) and/or yt-dlp `album` metadata mark a job as an album → no `.m3u`.
- [x] Final file paths are collected from yt-dlp itself (e.g. `--print after_move:filepath` or the `.info.json` files), not via `--exec "echo {} >> …"`.
- [x] `.m3u` is written as extended M3U: `#EXTM3U` header, then for each track `#EXTINF:<duration_secs>,<artist> - <title>` followed by the absolute path, in playlist order.
- [x] `.m3u` saved to a configurable playlists folder on the NAS, named after the playlist title (sanitized).
- [x] **Path mapping:** paths written into the `.m3u` and the `path` sent to Plex must be as the *Plex server* sees them (e.g. `/Volumes/data/Media/Music/…` on the Mac may be `/data/Media/Music/…` on the server). A configurable prefix rewrite handles this.
- [x] Triggers a Plex library scan for the music section and waits/retries until the new tracks exist before importing (otherwise Plex drops unmatched entries).
- [x] Imports via `POST {plex}/playlists/upload?sectionID=<id>&path=<m3u path>&X-Plex-Token=<token>`; re-running on the same playlist updates rather than duplicates (verify Plex behavior).
- [x] Failures in the Plex step do not fail the download; they surface as a warning with an AI-Ready Error Report.

**Known environment facts (from a Plex `/library/metadata/<id>` response):**
- Plex server is on a QNAP NAS. Music library: `librarySectionID="3"`, title `Music`.
- Plex-side path example: `/share/CACHEDEV1_DATA/Multimedia/Media/Music/Mrs. GREEN APPLE/POPS/08 - lulu..m4a` — matches the existing output template (`<artist>/<album>/<NN> - <title>.<ext>`).
- Path mapping (verified 2026-10-20 against `08 - lulu..m4a`): Mac `/Volumes/data/Media/Music/` (SMB share `data` on the QNAP) ⇄ Plex `/share/CACHEDEV1_DATA/Multimedia/Media/Music/`.
- `.m3u` must therefore be written with Plex-side paths, and the upload `path` param must also be the Plex-side path of the `.m3u`.
- Plex returns `duration` in milliseconds; `#EXTINF` wants seconds (use yt-dlp's `duration`, already in seconds).

**Config & secrets:**
- Plex base URL, section ID (default `3`), Mac→Plex path prefix pair, and the playlists folder are app settings.
- The Plex token is a secret: store it in the macOS Keychain (or the app's data dir, never in the repo), never log it, and redact it from AI-Ready Error Reports and request-URL logging.

**Open questions (triage):**
- Should re-running a playlist URL later only add new tracks (sync) or rebuild the `.m3u`?
