# 06 — External Metadata Enrichment

**What to build:** For standalone YouTube videos (Singles) that lack official album metadata, the backend automatically queries the MusicBrainz API to fetch and inject high-quality ID3 tags into the `.m4a` file before finalizing the download.

**Blocked by:** None — can start immediately (though likely built as a post-processing step after 04/05).

**Status:** ready-for-agent

- [ ] Identify when a downloaded track lacks album metadata.
- [ ] Query MusicBrainz API using the track title/artist.
- [ ] Inject the fetched metadata (Album, Year, etc.) into the `.m4a` file.
