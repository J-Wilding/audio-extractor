# 11 — Browser Extension (Manifest V3) to Send the Current Page

**What to build:** A minimal Chromium MV3 extension (in a new top-level `extension/` folder) with a toolbar button. On a YouTube Music song, album, or playlist page, clicking it sends the active tab's URL to the Browser Bridge and shows the job's status.

**Blocked by:** 10 — Browser Bridge Server

**Status:** done

- [x] `manifest.json` (MV3) with `activeTab` permission and `host_permissions` for `http://127.0.0.1:<port>/*` only.
- [x] Popup reads the page URL via `chrome.tabs.query({ active: true, currentWindow: true })` — **not** `window.location.href`, which would be the popup's own URL.
- [x] Popup classifies the URL before sending and shows it: song (`watch?v=`), album (`playlist?list=OLAK5uy_…` / `browse/MPREb…`), or playlist (`playlist?list=…`). For `watch?v=…&list=…`, the user chooses "this song" or "whole list".
- [x] Sends `POST /extract` with the `X-Bridge-Token` header, then polls `GET /jobs/:id` while the popup is open.
- [x] Clear states: app not running (connection refused), bad token (401), engine updating, queued, downloading, done, failed.
- [x] Options page to set port and paste the Bridge token (stored in `chrome.storage.local`).
- [x] README section on loading it unpacked (`chrome://extensions` → Developer mode → Load unpacked).
