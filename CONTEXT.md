# Domain Glossary

- **Audio Extractor**: The core application being built for the P0 prototype. A hyper-focused tool that takes a URL and extracts the highest quality audio, hiding the complexity of `yt-dlp` from the user. It is explicitly not a general-purpose video downloader.
- **Engine**: The underlying `yt-dlp` executable that powers the downloads. It is treated as an ephemeral dependency.
- **Engine Lock**: A concurrency mechanism in the backend that prevents download tasks from executing while the Engine is being downloaded or updated.
- **NAS Root**: The base directory on the network storage where all audio is saved (e.g., `/Volumes/data/Media/Music/`).
- **Metadata Routing**: The strategy of using `yt-dlp`'s output templates to automatically create Artist and Album subfolders inside the NAS Root without user intervention.
- **AI-Ready Error Report**: A pre-formatted diagnostic payload (including the input URL, raw stderr, and an LLM debugging prompt) generated upon extraction failure, designed to be instantly handed to an AI assistant by the maintainer.
