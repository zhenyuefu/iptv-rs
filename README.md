# iptv-rs

`iptv-rs` is a small Rust reimplementation of the aggregation-oriented parts of
[Guovin/iptv-api](https://github.com/Guovin/iptv-api).

Implemented:

- aggregate channels from local txt/m3u files and remote subscription txt/m3u files
- support per-subscription `UA="..."` syntax
- aggregate remote EPG XML/XML.GZ feeds with per-feed `UA="..."`
- generate txt and m3u playlist outputs
- resolve channel logos from local files first, then fall back to the configured remote logo base URL

Not implemented by design:

- RTMP/HLS push/streaming
- speed testing and FFmpeg probing
- web UI and GUI modes

## Quick Start

```sh
cargo run -- update --config config/config.ini
```

Outputs are written to `output/result.txt`, `output/result.m3u`, and
`output/epg/epg.xml` by default.

## Service Mode

Run the updater and expose generated output through HTTP:

```sh
cargo run -- serve --config config/config.ini
```

Default routes:

- `http://127.0.0.1:8080/` and `/txt` -> `output/result.txt`
- `/m3u` -> `output/result.m3u`
- `/epg` -> `output/epg/epg.xml`
- `/config/logo/<file>` and `/logo/<file>` -> local logo files
- `/info` -> route list using `PUBLIC_SCHEME`, `PUBLIC_DOMAIN`, and `PUBLIC_PORT`

## Docker

Compose deployment, matching the source project's deployment style:

```sh
docker compose up -d
```

Manual deployment:

```sh
docker pull zhenyuefu/iptv-rs:latest
docker run -d -p 80:8080 \
  -v /iptv-rs/config:/iptv-rs/config \
  -v /iptv-rs/output:/iptv-rs/output \
  -e PUBLIC_DOMAIN=your.domain.com \
  -e PUBLIC_PORT=80 \
  zhenyuefu/iptv-rs:latest
```

The container seeds missing files into `/iptv-rs/config`, updates on startup by
default, refreshes every `UPDATE_INTERVAL` hours, and serves generated output at
`/`, `/txt`, `/m3u`, and `/epg`.

Publish to DockerHub after logging in:

```sh
docker login
IMAGE=zhenyuefu/iptv-rs ./scripts/docker-publish.sh
```

For GitHub Actions publishing, set repository secrets `DOCKERHUB_USERNAME` and
`DOCKERHUB_TOKEN`, then run the `Publish Docker Image` workflow or push a `v*`
tag.

## Source Files

- `config/demo.txt` controls channel order and groups. It may also contain
  channel URLs.
- `config/local.txt` and files in `config/local/` are local live sources.
- `config/subscribe.txt` lists remote live sources, one URL per line.
- `config/epg.txt` lists remote EPG XML feeds, one URL per line.

Remote source and EPG entries support custom User-Agent values:

```text
https://example.com/live.m3u UA="My Player/1.0"
https://example.com/epg.xml.gz UA="My EPG Fetcher/1.0"
```

## Local-First Logos

Put logo files in `config/logo/`, for example:

```text
config/logo/CCTV-1.png
config/logo/湖南卫视.png
```

When generating M3U, `iptv-rs` first checks local logo files by exact channel
name, sanitized channel name, and EPG id. If no local file exists, it falls back
to `logo_url/<channel>.<logo_type>`.
