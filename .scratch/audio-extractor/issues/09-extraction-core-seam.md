# 09 — Decouple Extraction Core from the Tauri Command

**What to build:** Extract the body of `start_extraction` (`src-tauri/src/extractor.rs`) into a Tauri-agnostic core function, e.g. `extract(engine_path, url, on_progress: impl Fn(ProgressEvent))`, so that multiple entry points (the GUI and the upcoming Browser Bridge) can trigger the same extraction. The existing `start_extraction` Tauri command becomes a thin wrapper that forwards progress to `app.emit("extraction-progress", ..)`. No user-visible behavior changes.

**Blocked by:** None — can start immediately.

**Status:** done

- [x] Core extraction logic (NAS validation, engine invocation, output template, stdout/stderr streaming) lives in a function that does not take `AppHandle`.
- [x] Progress is reported through a callback/channel instead of direct `emit` calls.
- [x] The duplicated stdout/stderr line-reader threads are collapsed into one helper.
- [x] `start_extraction` keeps its current signature and the GUI behaves identically (manual smoke test: paste a URL, see progress, file lands on NAS).
- [x] A unit test exercises the core with a fake engine script (Engine Executor Seam from the spec) to verify progress lines and exit-code handling.
- [x] Concurrent jobs are serialized (a single job lock/queue shared by all entry points), respecting the existing Engine Lock.
