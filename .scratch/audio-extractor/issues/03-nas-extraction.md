# 03 — NAS Validation and Core Extraction Pipeline

**What to build:** The actual extraction logic. When the button is clicked, Rust verifies the NAS Root (`/Volumes/data/Media/Music/`) is mounted. If it is, it spawns the downloaded `yt-dlp` engine against the URL using our Metadata Routing template (handling Artist/Album and Singles) and embedding ID3 tags. Audio successfully appears on the NAS.

**Blocked by:** 02 — Engine Download, Update Lifecycle, and Lock

**Status:** ready-for-agent

- [x] Rust command handler receives the URL from the frontend.
- [x] Rust verifies that the NAS Root directory exists and is accessible.
- [x] If the NAS is disconnected, return an error state.
- [x] Rust spawns the `yt-dlp` subprocess using the correct output template: `%(artist|uploader)s/%(album|playlist|"Singles")s/%(playlist_index|track_number)s - %(title)s.%(ext)s`
- [x] Rust passes the flags to extract audio, use m4a, and embed metadata.
- [x] A successful download correctly routes the audio file to the NAS folder.
