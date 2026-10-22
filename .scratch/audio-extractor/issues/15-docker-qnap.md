# 15 — Docker Image + Compose for QNAP Container Station

**What to build:** A small image that runs the server with ffmpeg, plus a compose file the maintainer can deploy on the QNAP.

**Blocked by:** 13

**Status:** ready-for-human

- [x] Multi-stage `server/Dockerfile`: `rust:1-bookworm` builder, then a `debian:bookworm-slim` runtime with `ffmpeg ca-certificates python3 curl gosu`.
- [x] Runs as a non-root user. `PUID`/`PGID` are configurable so file ownership matches the Music share.
- [x] `docker-compose.yml`: mounts `/share/CACHEDEV1_DATA/Multimedia/Media/Music:/music` and `./data:/data`, publishes `47821:47821`, sets `restart: unless-stopped`, reads env from `.env`.
- [x] Commit `.env.example`. Add `.env` to `.gitignore`.
- [x] `HEALTHCHECK` hitting `/health`.
- [x] GitHub Actions: build with buildx for `linux/amd64` + `linux/arm64` and push to `ghcr.io/j-wilding/audio-extractor-server` on `main` and `v*` tags. Smoke test checks `/health` and `ffmpeg -version`.
- [x] `server/README.md`: Container Station deploy steps, how to find the Plex token, update procedure.
- [ ] Verified on the real NAS: a playlist download lands in the right folder with the right owner, and the `.m3u` imports into Plex.
