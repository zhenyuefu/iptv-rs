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
- `config/local_sources.txt`: local live-source files with per-file options.
- `config/subscribe.txt`: remote live-source subscriptions.
- `config/epg.txt`: remote EPG XML/XML.GZ subscriptions.
- `config/logo/`: local channel logos.

Remote source and EPG entries support custom User-Agent values:

```text
https://example.com/live.m3u UA="My Player/1.0"
https://example.com/epg.xml.gz UA="My EPG Fetcher/1.0"
```

Live-source subscription entries and `config/local_sources.txt` entries also
support IPTV source labels:

```text
https://example.com/live.m3u IPTV="sh-unicom" UA="My Player/1.0"
config/local/sh-unicom.m3u IPTV="sh-unicom"
/data/public.txt
file:///data/zj-telecom.txt IPTV="zj-telecom"
```

An explicit `IPTV=` label means the source is an operator IPTV source and is
only kept when it matches `iptv_source_filter`, for example
`iptv_source_filter = sh-unicom`. Unlabeled ordinary internet sources are not
filtered. Matching IPTV sources are placed first; if `iptv_source_prefer` is
empty, `iptv_source_filter` is also used as the preferred order.

Local live sources are loaded only from `config/local_sources.txt`; files under
`config/local/` are not scanned automatically. Request `/txt?iptv=sh-unicom` or
`/m3u?iptv=sh-unicom` to dynamically filter and prefer matching IPTV sources.
The query parameter aliases `source` and `iptv_source` are also accepted.

## Logos

Put logo files in `config/logo/`, for example:

```text
config/logo/CCTV-1.png
config/logo/湖南卫视.png
```

When generating M3U, local logo files are preferred. If no local logo is found,
`logo_url` and `logo_type` from `config/config.ini` are used as the fallback.
