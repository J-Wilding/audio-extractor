# 14 — Job Queue, Persistent Job Log and Cursor API

**What to build:** Jobs queue up instead of only running one at a time behind the lock. Every Engine line is kept in a per-job log that clients can read incrementally with a cursor, so a client that reconnects never misses output. This fixes the root cause of "closing the popup loses the log".

**Blocked by:** 13

**Status:** done

- [x] FIFO queue with a single worker. States are `queued → running → succeeded | failed | cancelled`.
- [x] Per-job log: `VecDeque<{ seq, ts, line }>` ring buffer capped at 2,000 lines. `seq` keeps increasing even when old lines are dropped.
- [x] `POST /jobs` `{ url, mode }` → `Job`. `GET /jobs` (newest first, summaries). `GET /jobs/{id}`. `GET /jobs/{id}/log?since=N` → `{ lines, next, done, truncated }`.
- [x] `POST /jobs/{id}/cancel`: removes a queued job from the queue, or kills the running Engine child process and its children (ffmpeg).
- [x] Parse `[download] Downloading item X of Y` and `%` into structured `progress { item, total, percent }` on the job, so clients don't have to parse the log.
- [x] Keep the last 50 jobs (with their logs) in `DATA_DIR/jobs.json`. Write it atomically (temp file + rename). On startup, mark any job that was `running` as `failed (interrupted)`.
- [x] Keep `/extract` and `/jobs/{id}` response shape as compat aliases until issue 17 ships.
- [x] Tests: queue order, cursor semantics including truncation, cancel (queued and running, using the fake Engine), persistence round-trip, restart recovery.
