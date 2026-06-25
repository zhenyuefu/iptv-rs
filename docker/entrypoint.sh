#!/bin/sh
set -eu

: "${APP_WORKDIR:=/iptv-rs}"
: "${CONFIG_PATH:=$APP_WORKDIR/config/config.ini}"

mkdir -p "$APP_WORKDIR/config" "$APP_WORKDIR/output"

for file in /iptv-rs-config/*; do
    [ -e "$file" ] || continue
    filename="$(basename "$file")"
    target="$APP_WORKDIR/config/$filename"
    if [ ! -e "$target" ]; then
        cp -R "$file" "$target"
    fi
done

exec iptv-rs serve --config "$CONFIG_PATH"
