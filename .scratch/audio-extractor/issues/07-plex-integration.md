# 07 — Plex API Integration for Manual Library Scans

**What to build:** The UI includes an "Update Plex" button or menu item that triggers a library scan via the Plex API. This ensures any newly extracted audio files on the NAS are immediately picked up and available in the Plex interface.

**Blocked by:** None — can start immediately.

**Status:** ready-for-agent

- [ ] The backend implements a route to hit the Plex API's library scan endpoint (requiring config for Plex URL and Token).
- [ ] The frontend provides a clear, accessible button (e.g. in the menu bar) to trigger the scan.
- [ ] The UI provides visual feedback (success/error states) when the scan is initiated.
