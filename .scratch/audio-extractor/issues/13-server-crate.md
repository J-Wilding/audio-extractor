# 13 — Headless `server/` Crate (Extract Core from Tauri)

**What to build:** A standalone Rust binary (`server/`) that runs the existing extraction, M3U and Plex logic with no Tauri dependency. It's configured entirely through env vars. See spec v2 § Backend Service.

**Blocked by:** —

**Status:** done

- [x] New crate `server/` with `axum` + `tokio` (multi-thread runtime). Port `extractor.rs`, `plex.rs` and the job model and handlers from `bridge.rs`.
- [x] Replace the `BridgeHost` trait with plain `AppState { cfg, engine, jobs }`.
- [x] `Config::from_env()` covering `BRIDGE_TOKEN` (required), `PLEX_URL`, `PLEX_TOKEN`, `PLEX_SECTION_ID`, `MUSIC_ROOT`, `PLEX_MUSIC_ROOT`, `PLAYLISTS_DIR`, `DATA_DIR`, `PORT`. Fail fast with a clear message if anything is missing or invalid. Never log the token values.
- [x] Bind `0.0.0.0:$PORT` (default 47821). Keep token auth and the `chrome-extension://`-only CORS policy.
- [x] Engine bootstrap: download `yt-dlp_linux` into `DATA_DIR` if missing, run `-U` at startup and every 24h, holding the Engine Lock while it runs.
- [x] Drop the macOS-specific `--paths temp:` / `--no-part` workarounds. Use `DATA_DIR/tmp` as the temp path.
- [x] Port all existing `bridge.rs` / `extractor.rs` / `plex.rs` tests. `cargo test --manifest-path server/Cargo.toml` passes.
- [x] `src-tauri/` is left untouched.
