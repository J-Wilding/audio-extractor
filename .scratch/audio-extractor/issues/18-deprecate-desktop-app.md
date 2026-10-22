# 18 — Deprecate and Remove the Tauri Desktop App

**What to build:** Retire `src-tauri/` and the Vite frontend once v2 is running on both machines.

**Blocked by:** 15, 17 (deployed and used for about a week)

**Status:** ready-for-human

- [x] Now: deprecation notice at the top of `README.md` pointing to `server/` + `extension/`. Freeze: bug fixes only.
- [x] Update `CONTEXT.md`: Browser Bridge → Backend Service. Add Job Log, Side Panel. Retire the "app must be open" constraint.
- [ ] Later: tag the final desktop release (`desktop-final`), then delete `src-tauri/`, `src/`, `index.html`, `vite.config.ts` and the Tauri npm deps.
- [ ] Remove the old `/extract` compat alias from the server.
