# AGENTS.md — Archived Agent Notes

Archived notes are frozen historical snapshots, not current authority. Never edit, reformat, translate, repair, delete or move one; use an active Agent Note or current documentation for new decisions and facts.

Archive only with `node .agents/bin/agent-notes.js archive <note>`: it relocates the note, inserts the `Archived: YYYY-MM-DD` line below `Status: implemented`, and appends its hash to `manifest.json`. Do not edit `manifest.json` by hand; `node .agents/bin/agent-notes.js verify` rejects changed or missing sealed notes. Links out of archived notes are never checked or repaired.
