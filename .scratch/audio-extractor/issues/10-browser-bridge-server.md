# 10 — Browser Bridge: Local HTTP Server Inside the Tauri App

**What to build:** While the app is open, it runs a small HTTP server (e.g. `axum` on `tauri::async_runtime`) bound to `127.0.0.1` only. A browser extension can submit a URL for extraction and poll its status. Jobs submitted via the Bridge also appear in the app window exactly like pasted URLs. Requiring the app to be open is an accepted constraint (no daemon, no deep links).

**Blocked by:** 09 — Extraction Core Seam

**Status:** done

- [x] Server starts in Tauri `setup` and binds to `127.0.0.1:<port>` (default e.g. `47821`, configurable); never `0.0.0.0`.
- [x] `POST /extract` with JSON `{ "url": "..." }` validates the URL is YouTube / YouTube Music, enqueues a job via the core from 09, and returns `{ "job_id": "..." }`.
- [x] `GET /jobs/:id` returns status (`queued | running | succeeded | failed`), latest progress line, and an error message on failure.
- [x] `GET /health` returns app/engine readiness so the extension can show "app not running" vs "engine updating".
- [x] Requests require a shared secret header (e.g. `X-Bridge-Token`) generated on first run and shown/copyable in the app UI; requests without it get `401`.
- [x] CORS allows the extension origin (`chrome-extension://<id>`) and the token header; preflight `OPTIONS` handled.
- [x] GUI reflects Bridge-submitted jobs (progress bar, success/error, AI-Ready Error Report).
- [x] Integration test hits the router in-process (e.g. `tower::ServiceExt::oneshot`) for auth, validation, and job lifecycle with a fake engine.

**Implementation notes:**
- Code: `src-tauri/src/bridge.rs` (router, job registry, token), wired up in `src-tauri/src/lib.rs` (`TauriBridgeHost`, `get_bridge_info`).
- Port: default `47821`, override with env var `YTDLM_BRIDGE_PORT` (no settings UI yet).
- Token: 48-hex-char secret in `<app data dir>/bridge_token` (mode 0600), shown and copied to the clipboard via the **Browser Bridge** menu item.
- Events: Bridge jobs emit `bridge-job` (status changes) and reuse `extraction-progress` (Engine output), so the GUI progress bar works for both.
- CORS allows only `chrome-extension://` / `moz-extension://` origins; preflight is answered before the token check.
