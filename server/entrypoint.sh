#!/bin/sh
set -e

PUID="${PUID:-0}"
PGID="${PGID:-0}"
umask 0002

mkdir -p /data /data/tmp

if [ "$(id -u)" = "0" ] && [ "$PUID" != "0" ]; then
    chown -R "$PUID:$PGID" /data
    exec gosu "$PUID:$PGID" /usr/local/bin/audio-extractor-server
fi

exec /usr/local/bin/audio-extractor-server
