//! Audio Extractor backend service (spec v2): a headless HTTP API that queues
//! YouTube audio downloads, runs yt-dlp, and imports playlists into Plex.

pub mod api;
pub mod config;
pub mod engine;
pub mod extractor;
pub mod jobs;
pub mod plex;
pub mod worker;
