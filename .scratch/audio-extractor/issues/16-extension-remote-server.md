# 16 — Extension: Remote Server Settings + Shared API Client

**What to build:** Point the extension at the NAS instead of `127.0.0.1`. Factor the fetch and auth code into one module.

**Blocked by:** 14 (API shape)

**Status:** done

- [x] `options.html`: server URL + token fields and a "Test connection" button (unauthenticated `/health`, then authenticated `GET /jobs`), with clear errors for unreachable / bad token / CORS.
- [x] Manifest: add `optional_host_permissions: ["http://*/*","https://*/*"]` (plus default host permissions for `127.0.0.1` and `10.0.0.69`). Call `chrome.permissions.request({ origins: [<server origin>/*] })` when settings are tested/saved.
- [x] `api.js` shared by popup, side panel, options and background: `getJobs`, `getJob`, `getLog(id, since)`, `submitJob(url, mode)`, `cancelJob(id)`, `scanPlex()`, with typed errors.
- [x] Rename the extension and update its description (it no longer requires a desktop app). Bump the version to 0.2.0.
