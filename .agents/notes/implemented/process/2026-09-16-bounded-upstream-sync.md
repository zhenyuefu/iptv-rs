# Agent Note: Adopt Agent Notes and bounded upstream synchronization

Status: implemented

## Problem

The upstream IPTV project has expanded substantially. Following every upstream
feature would change this repository's purpose and maintenance cost, while leaving
its channel data untouched would reduce aggregation coverage. Scope boundaries
and the reasons for selective synchronization need a durable home beyond a task
conversation.

## Decision

The repository uses [Agent Notes](../../README.md) with a vendored, dependency-free
[validator](../../../bin/agent-notes.js). Non-trivial changes update a decision
record alongside code; the root [instructions](../../../../AGENTS.md) carry the
workflow. English follows the existing repository documentation.

Upstream synchronization prioritizes channel aliases and aggregation data while
preserving the core scope: playlist aggregation, filtering, ordering, EPG, output,
and the existing file service. UI/GUI, speed testing, FFmpeg probing, local
RTMP/HLS streaming and speed-derived filters remain outside the scope.

The data baseline is the local Guovin/iptv-api checkout at commit
`27d7f8463048266eac6d73fd02c96fe1d24023c8` (2026-09-11), not a claim about the
latest remote revision. [Aliases](../../../../config/alias.txt), the
[channel template](../../../../config/demo.txt), and 21 PNGs in
[local logos](../../../../config/logo) are copied from that baseline. The default
EPG URL is added to [the EPG list](../../../../config/epg.txt), preserving its
UA documentation. Blacklist and whitelist already match. Subscription and local
source differences are comments only, so their Rust-specific examples remain.
The runtime INI and IPTV source-label configuration remain repository-owned.

[AliasMatcher](../../../../src/rules.rs) uses fancy-regex for alias patterns,
including upstream negative lookahead assertions. Invalid custom expressions
emit a diagnostic and are skipped; a bounded matching error is treated as a
non-match. Exact aliases still take precedence over patterns, followed by the
normalized-name fallback. Generated normalized primary names live in a separate
map so they cannot overwrite explicit aliases (for example, normalizing
`翡翠台4K` must not steal `翡翠` from `翡翠台`). This is targeted compatibility, not a claim
that every Python regular expression is supported.

## Alternatives considered

**Copy all upstream configuration and features.** This conflicts with the user's
core-function focus and would import settings for unsupported subsystems.

**Copy data without changing the matcher.** The previous regex engine silently
skipped lookahead patterns; a larger catalog alone would leave regional and
number-boundary rules inactive. Rewriting upstream patterns into a separate Rust
catalog would create an ongoing translation burden, so the catalog stays intact.

**Keep rationale only in task history.** This would not give future repository
work a locally verifiable scope and decision record. Agent Notes are explicitly
requested for that purpose.

## Consequences

Channel coverage and local logo availability increase. The expanded default
template changes groups and ordering, and the new default EPG source adds a
network request when EPG updates are enabled. Existing deployed config files are
not overwritten by the container's missing-file seeding behavior; operators must
merge these defaults into an existing mounted config directory themselves.

The matcher adds a backtracking-capable dependency with its default resource
limit. Regression tests load the shipped catalog, compile every pattern, check
all exact alias owners and exercise CCTV regional/number distinctions. These
checks protect compatibility without pulling in upstream UI or probing systems.
`cargo fmt --check`, `cargo check --locked`, `cargo test --locked` and
`node .agents/bin/agent-notes.js verify` validate the change. CI wiring is left
for a separate requested change; no commits are created by initialization.
