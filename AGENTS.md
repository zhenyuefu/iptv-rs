# AGENTS.md

## Project Notes

`iptv-rs` is a Rust reimplementation of the aggregation-oriented parts of
`Guovin/iptv-api`. The source project is expected to be available at
`../iptv-api` when comparing behavior locally.

The intended alignment excludes local RTMP/HLS push/streaming and speed-test
features.

## Implemented Scope

- Aggregate channels from local txt/m3u files and remote subscription txt/m3u files.
- Match channels through `config/alias.txt`, including `re:` regular-expression aliases.
- Apply `config/blacklist.txt` URL keyword filtering.
- Apply `config/whitelist.txt` exact and keyword URL retention.
- Support per-subscription `UA="..."` syntax.
- Use template-first output ordering.
- Support optional unmatched and empty categories.
- Reuse historical `output/result.txt` entries when `open_history = True`.
- Support source and IP ordering preferences.
- Support update-time rows and optional URL info suffixes.
- Auto-disable failed or empty subscription/EPG sources when `open_auto_disable_source = True`.
- Aggregate remote EPG XML/XML.GZ feeds with per-feed `UA="..."`.
- Generate txt and m3u playlist outputs.
- Resolve channel logos from local files first, then fall back to configured remote logo base URL.

## Out Of Scope By Design

- RTMP/HLS push or local streaming.
- Speed testing and FFmpeg probing.
- Web UI and GUI modes.
- Speed-derived filters such as bitrate/resolution validation, geographic ISP probing,
  freeze/supply caches, and speed/statistics logs.

## Development Commands

Format:

```sh
cargo fmt
```

Check:

```sh
cargo check
```

Test:

```sh
cargo test
```

Run update:

```sh
cargo run -- update --config config/config.ini
```

Run service:

```sh
cargo run -- serve --config config/config.ini
```

## Important Files

- `src/main.rs`: update orchestration.
- `src/config.rs`: config and environment overrides.
- `src/playlist.rs`: txt/m3u parsing, alias-aware aggregation, filtering, ordering.
- `src/rules.rs`: alias, blacklist, and whitelist loading/matching.
- `src/output.rs`: txt/m3u output writers.
- `src/epg.rs`: EPG aggregation and filtering.
- `src/service.rs`: built-in HTTP file service.
- `src/source_list.rs`: subscription/EPG source-list parsing and auto-disable edits.
