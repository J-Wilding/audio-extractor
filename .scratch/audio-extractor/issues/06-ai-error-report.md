# 06 — Implement AI-Ready Error Report for Failed Extractions

**What to build:** When an extraction fails, the application generates a pre-formatted diagnostic payload containing the input URL, raw stderr output from yt-dlp, and a prompt designed for an LLM debugger. The UI presents this payload to the user (e.g. via a "Copy Error for AI" button) for immediate troubleshooting.

**Blocked by:** None — can start immediately.

**Status:** ready-for-agent

- [ ] On extraction failure, the Rust backend passes the full stderr, URL, and exit code to the frontend.
- [ ] The frontend compiles this data into a structured prompt/payload.
- [ ] The UI displays a clear error state with a button to copy the AI-ready payload to the clipboard.
