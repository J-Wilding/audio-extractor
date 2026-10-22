# 17 — Extension: Persistent Side Panel, Pop-out Window and Badge

**What to build:** A persistent place to watch jobs, so closing the popup no longer loses progress.

**Blocked by:** 14, 16

**Status:** done

- [x] Manifest: `"side_panel": { "default_path": "sidepanel.html" }` and the `sidePanel` + `alarms` permissions.
- [x] `sidepanel.html/js`:
  - Job list (status pill, title/URL, "Track X/Y", progress bar).
  - Selecting a job shows its log, which auto-scrolls unless the user has scrolled up.
  - Cancel button on queued and running jobs.
  - Polls `/jobs` every 2s and `/jobs/{id}/log?since=` every 1s. Pauses when `document.hidden`.
  - On open, loads the full log for the selected/active job from `since=0`. This is the catch-up behavior.
- [x] Pop-out: `sidepanel.html?popout=1` opened with `chrome.windows.create({ type: "popup", width: 440, height: 680 })`. The window ID is stored in `chrome.storage.session`, and an existing window is focused instead of opening a duplicate.
- [x] Popup: after submitting, shows "Queued Job #N" with "Open Side Panel" (`chrome.sidePanel.open({ windowId })`) and "Pop-out Log Window", and automatically reconnects to any running job if reopened.
- [x] Background service worker: a `chrome.alarms` poll (every 30s) + instant message refresh sets the action badge to the active job count.
