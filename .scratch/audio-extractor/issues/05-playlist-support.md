# 05 — Playlist Support & Batch Extraction

**What to build:** The user can paste a YouTube playlist URL and the application successfully routes all tracks to the NAS while the UI clearly indicates batch progress (e.g., "Downloading Track X of Y").

**Blocked by:** 04 — Progress Streaming

**Status:** ready-for-agent

- [ ] `yt-dlp` properly parses and iterates over playlist URLs.
- [ ] The UI visually indicates that a batch extraction is occurring and which track is currently downloading.
- [ ] Output files are correctly numbered and stored in their respective Album/Playlist folder on the NAS.
