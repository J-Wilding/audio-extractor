Status: ready-for-agent

## Problem Statement

The `yt-dlp` CLI is a powerful tool for downloading high-quality audio from YouTube, but its command-line options are complex and difficult to remember. For a non-technical user, executing terminal commands to extract audio and correctly route it to a specific directory on a Network Attached Storage (NAS) drive is high-friction, intimidating, and error-prone.

## Solution

A fast, lightweight, purpose-built desktop application (Audio Extractor) that acts as a frictionless wrapper around `yt-dlp`. It provides a clean, single-input GUI for extracting audio. The application automatically manages the underlying Engine (`yt-dlp`), validates the NAS Root connection, intelligently routes the output into Artist and Album subfolders using Metadata Routing, embeds ID3 tags, and abstracts all complex failure states into AI-Ready Error Reports for the maintainer.

## User Stories

1. As a non-technical user, I want to paste a YouTube URL into a classic text input field, so that I can easily initiate an audio extraction without knowing CLI flags.
2. As a non-technical user, I want to see a single global progress bar during download, so that I can easily understand the status without being overwhelmed by verbose granular logs.
3. As a non-technical user, I want the app to automatically file my downloaded audio into Artist and Album folders on our NAS.
4. As a non-technical user, I want single-song downloads to be automatically placed in a `Singles` folder.
5. As a non-technical user, I want the downloaded files to automatically contain embedded ID3 metadata.
6. As a non-technical user, I want to be visually warned if the NAS is disconnected before downloading.
7. As a non-technical user, I want to see a friendly "Oops" message when a download fails.
8. As a non-technical user, I want to click a single button to copy an error report when something breaks.
9. As the app maintainer, I want error reports to be "AI-Ready" (containing the URL, stderr, and a debug prompt).
10. As a user, I want the app to start up instantly without blocking the UI.
11. As a user, I want the underlying extraction Engine to update itself asynchronously in the background.
12. As a user, I want the app to temporarily queue my download if the Engine is currently updating (via an Engine Lock).
13. As a user, I want the app to work frictionlessly without requiring me to install browser cookies.

## Implementation Decisions

- **Stack:** Rust backend with Tauri frontend (ADR-0001).
- **Engine Management:** The `yt-dlp` executable (the Engine) is downloaded on first run and self-updated via `yt-dlp -U` (ADR-0002).
- **Engine Lock:** Async background thread with Engine Lock (ADR-0003).
- **NAS Validation & Metadata Routing:** Validates `/Volumes/data/Media/Music/` and uses fallback template syntax (ADR-0004).
- **Stateless Auth:** Dropped the `--cookies-from-browser` requirement.
- **AI-Ready Error Reporting:** Formats stderr into AI payloads (ADR-0005).

## Testing Decisions

- **Tauri IPC Boundary:** Primary seam.
- **Engine Executor Seam:** Mocking the subprocess to inject stderr without hitting network.

## Out of Scope
- Intelligent external metadata enrichment (v2).
- Video downloading.
