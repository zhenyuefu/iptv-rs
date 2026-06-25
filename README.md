# iptv-rs

IPTV live-source and EPG aggregator.

## Quick Start

```sh
cargo run -- update --config config/config.ini
```

Default outputs:

- `output/result.txt`
- `output/result.m3u`
- `output/epg/epg.xml`

## Service Mode

Run the updater and serve generated files over HTTP:

```sh
cargo run -- serve --config config/config.ini
```

Default routes:

- `/`, `/txt`, `/content` -> `output/result.txt`
- `/m3u` -> `output/result.m3u`
- `/epg` -> `output/epg/epg.xml`
- `/config/logo/<file>`, `/logo/<file>` -> local logo files
- `/info` -> public route list
- `/health` -> health check

## Docker

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

## Config Files

- `config/config.ini`: runtime settings.
- `config/demo.txt`: channel template, output order, and groups.
- `config/alias.txt`: channel aliases. Entries prefixed with `re:` are regular expressions.
- `config/blacklist.txt`: URL keyword blacklist.
- `config/whitelist.txt`: exact or keyword URL whitelist.
- `config/local.txt`: local live sources.
- `config/local/`: additional local txt/m3u source files.
- `config/subscribe.txt`: remote live-source subscriptions.
- `config/epg.txt`: remote EPG XML/XML.GZ subscriptions.
- `config/logo/`: local channel logos.

Remote source and EPG entries support custom User-Agent values:

```text
https://example.com/live.m3u UA="My Player/1.0"
https://example.com/epg.xml.gz UA="My EPG Fetcher/1.0"
```

Live-source subscription entries also support IPTV source labels:

```text
https://example.com/live.m3u IPTV="home" UA="My Player/1.0"
https://example.com/backup.txt IPTV=backup
```

Local files use their filename stem as the IPTV source label, for example
`config/local/home.txt` is labeled `home`. Set `iptv_source_prefer` in
`config/config.ini`, or request `/txt?iptv=home` and `/m3u?iptv=home`, to place
matching source URLs first for each channel. The query parameter aliases
`source` and `iptv_source` are also accepted.

## Logos

Put logo files in `config/logo/`, for example:

```text
config/logo/CCTV-1.png
config/logo/湖南卫视.png
```

When generating M3U, local logo files are preferred. If no local logo is found,
`logo_url` and `logo_type` from `config/config.ini` are used as the fallback.
