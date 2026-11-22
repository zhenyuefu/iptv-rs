# AGENTS.md

## Project Notes

`iptv-rs` is a Rust reimplementation of the aggregation-oriented parts of
`Guovin/iptv-api`. The source project is expected to be available at
`../iptv-api` when comparing behavior locally.

The optional HTTP pull relay serves existing upstream media. RTMP/HLS ingest,
push, transcoding and speed testing remain outside upstream alignment.

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
- Optionally aggregate named HTTP sources on separate relay listeners and forward HLS or MPEG-TS to LAN and tailnet players.
- Resolve channel logos from local files first, then fall back to configured remote logo base URL.

## Out Of Scope By Design

- RTMP/HLS ingest, push, transcoding and locally generated media streams.
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
- `src/relay/`: asynchronous HTTP relay, source catalogues, and HLS resource rewriting.
- `src/source_list.rs`: subscription/EPG source-list parsing and auto-disable edits.

## Agent Notes

[Agent Notes](.agents/notes/README.md) governs repository documents, plans, research, analysis, designs, guides and decisions.

- Update authoritative content in the same change as substantive work; do not add a duplicate ADR.
- Search CE type=note or `node .agents/bin/agent-notes.js related <keywords>`, then read evidence to establish ownership.
- Select one of six kinds by purpose. Small topics need not split. Use new --kind, show/write and transition; see the governance guide.
- Distinguish evidence scope from shipped status. Archives and rejected decisions are frozen. Finish with `node .agents/bin/agent-notes.js verify`.
