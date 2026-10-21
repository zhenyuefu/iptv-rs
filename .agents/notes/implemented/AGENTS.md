# AGENTS.md — Implemented Agent Notes

These notes describe shipped decisions. Follow the [Agent Note format](../README.md#the-file-format).

## Keep an implemented note current with what actually shipped

Keep paths, symbols, defaults and mechanisms current in the same change that alters them. Rewrite stale facts in place; do not append change history — git holds that.

When a shipped note is unlikely to guide future work, archive it with `node .agents/bin/agent-notes.js archive <note>` instead of continuing to maintain it.

### This is not a license to rewrite the *decision*

Update the factual realization in place. A reversal of the decision or its rationale requires a new Agent Note and a cross-link; a fully superseded old note may be deleted only through the consolidation rule in the [Agent Note rules](../README.md#when-to-write-one).
