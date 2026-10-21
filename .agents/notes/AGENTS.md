# AGENTS.md — Agent Notes

Agent Notes are RFCs written by agents: durable proposals and decision records that preserve rationale, alternatives, consequences and required verification. Follow the [Agent Note rules](README.md).

**Every new Agent Note triggers a supersession check.** Before writing, run `node .agents/bin/agent-notes.js related <keywords>` and read the active notes covering the same decision or mechanism. Update the owner instead of duplicating it; archive or fold in fully superseded implemented notes in the same change; keep partial supersessions active and cross-linked.

**Before finishing a change,** run `node .agents/bin/agent-notes.js verify`. It must pass.

Files under [`archived/`](archived/AGENTS.md) are frozen historical snapshots: never edit them or treat them as current authority.
