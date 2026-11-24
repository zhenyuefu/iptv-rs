---
schema: "agent-note/2"
id: "usage"
kind: "guide"
state: "current"
title: "iptv-rs"
topic: "usage"
owns: ["operations/usage"]
migration_source: "README.md"
migration_sha256: "f1467bc3ef4caf77a56036d49cee7031c2139fefc6ab558f372e5f8f5bf0ba10"
---

# iptv-rs

## 适用范围与前提

频道聚合服务的构建、配置和运行入口。

2026-10-23 迁移自 `README.md`。保留原文的日期、样本、版本、反例、未知项和状态；本次只迁移文档，没有重新运行原文中的实验、设备操作或部署。相对命令以原项目目录为工作目录。历史上已不可用的引用使用 `missing:` URI 保留原始目标，不能作为现存文件证据。

## 操作步骤

按下列原文中的前提、操作顺序与命令执行；具体版本和回滚条件随正文保留。

## 成功判据与故障处理

以原文记录的检查结果、限制、故障处理和回滚条件为准；迁移本身不证明外部环境仍满足这些条件。

## 原始正文

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

## Optional HTTP relay for LAN and Tailscale

The same `serve` process runs the scheduled channel/EPG aggregator on its regular HTTP port and optional asynchronous media listeners on separate addresses. Enable the listeners with a JSON file. A NAS-specific private copy can live at `config/relay.json`; Git and the Docker build context ignore this file. Mount it with your runtime configuration. The tracked [example](../../../../config/relay.example.json) shows three named sources and separate LAN, tailnet, and Docker-gateway listeners.

```sh
cargo run --release --locked -- serve --config config/config.ini --relay-config config/relay.json
```

- On the aggregation port, `/m3u`, `/txt`, and `/epg` retain template ordering, aliases, source filtering and scheduled output. For a full LAN playlist with relayed Unicom channels, add the relay's LAN or Docker-gateway `/unicom.txt` URL to `config/subscribe.txt` in the desired source section. Other LAN subscriptions continue through the usual aggregator.
- On a relay listener, `/playlist.m3u` and `/m3u` combine only the named sources in JSON order. Each source has `/<id>.txt` and `/<id>.m3u`. Separate groups keep alternate feeds for the same station selectable. A listener's configured `public_base` determines channel URLs, including a Docker-gateway listener that advertises a LAN address.
- A group specifies a TXT or M3U catalogue, a channel format (`hls` or `ts`), and exact allowed HTTP(S) origins or HTTP port-80 IP networks. Approve destinations used by both channels and their nested resources. Channel IDs follow the existing relay's SHA-256 of channel name and upstream path. HLS child manifests, segments, keys and maps pass through the relay. Media transfers stream with backpressure, HTTP ranges and bounded resource storage. Source catalogues refresh on demand after one hour and retain the last successful result on a transient failure. A cold source failure omits its group from the combined list, and a completely unavailable list returns HTTP 503.
- On Linux/NAS with these host IP listeners, run the Docker image with host networking. Select unused service and relay ports for a side-by-side test. The aggregation port is controlled by `NGINX_HTTP_PORT` in the image, and relay ports by `listeners[].bind`. Use dedicated Rust config and output mounts. For example, from the project root on the Linux host:

```sh
docker build -t iptv-rs:relay .
docker run --name iptv-rs-relay --network host --restart unless-stopped \
  -e NGINX_HTTP_PORT=7080 \
  -v "$PWD/config:/iptv-rs/config" -v "$PWD/output:/iptv-rs/output" \
  iptv-rs:relay serve --relay-config /iptv-rs/config/relay.json
```

On the production NAS, iptv-api serves the LAN catalogue on 7080, the standalone Rust relay listens on 7091, and a separate `iptv-rs serve` instance publishes the three-source APTV catalogue and XMLTV guide on 7092. Use the aggregation endpoint for template categories and channel aliases. The Rust implementation was validated locally using the real Unicom, AC5 and YSP sample channels, with the AC5 upstream reached through the existing NAS relay. The loopback integration test exercises the normal updater, two catalogue formats, HLS redirect/child/key rewriting, byte ranges, destination checks and concurrent MPEG-TS transfers.

### Native network playlists

For a shared subscription catalogue, label the upstream entries in `subscribe.txt` with `Source="..."` matching the relay source IDs. Add a `profiles` array alongside `sources` and `listeners` in the relay JSON. For example:

```json
{
  "name": "home-a",
  "source_order": ["a", "b"],
  "relay_sources": ["b"],
  "relay_base": "http://192.0.2.10:7091",
  "epg_url": "http://192.0.2.10:7092/epg"
}
```

Each array entry publishes `/output/<name>.m3u` on the aggregation port. Set the public relay and optional XMLTV URLs to addresses reachable by that network's players. A reverse proxy can also expose these endpoints on a router. The updater uses the existing template, aliases, logos, source preference and URL limit. Only streams from listed source IDs appear. Direct stream URLs stay intact, including their query parameters. Relayed streams use the original upstream channel name and URL path to find an actual entry in that source's allowed relay catalogue.

Profiles refresh on the normal aggregation schedule and on `iptv-rs update --config config/config.ini --relay-config config/relay.json`. The one-shot command does not bind listeners. A complete profile replaces its previous file atomically. Missing sources or unmatched relay entries leave the previous profile available and report an update error.

### Standalone relay

Use `iptv-rs relay --relay-config /etc/iptv-rs/relay.json` when another service already publishes the aggregated channel list. This command starts only the configured relay listeners. It runs without a config.ini, output directory or updater. For the standalone image, build the `relay` Dockerfile target and provide the JSON file on a read-only bind mount. On Linux, host networking allows binding the individual LAN, tailnet and Docker-gateway addresses.

```sh
docker build --target relay -t iptv-rs:relay-only .
docker run -d --name iptv-rs-relay --restart unless-stopped --network host \
  --read-only --cap-drop ALL --security-opt no-new-privileges \
  -v "$PWD/config/relay.json:/etc/iptv-rs/relay.json:ro" iptv-rs:relay-only
```

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
