# Agent Notes: repository knowledge

Search CE with cwd, type=note and history=true, then read relevant owners. If unavailable,
report it and use related --lexical. Update canonical content alongside substantive changes;
a small topic needs one Note.

Organize content under topics/<topic>/ with agent-note/2 metadata. Choose kind by purpose:

| kind | Owns | States |
|---|---|---|
| decision | Choice, rationale and trade-offs | proposed / implemented / rejected |
| plan | Tasks, dependencies and progress | planned / active / completed / cancelled |
| research / analysis | Scoped evidence, reasoning and conclusions | draft / completed / cancelled |
| design | Contracts and verification | draft / accepted / withdrawn |
| guide | Steps and troubleshooting | draft / current / obsolete / withdrawn |

A proposal is a decision in proposed state; architecture is a decision class. Proposals own
the choice and rationale. Split out a plan only when execution needs independent tracking;
link the proposal/design instead of copying contracts or acceptance details. Completed plans
or investigations and accepted designs do not prove shipped behavior.

Prefer the CE MCP notes_* tools: notes_get returns content, allowed transitions and an edit_id;
notes_create writes the complete Note; notes_write/notes_transition/notes_relocate take the edit_id.
Reread conflicts; read_id is only for evidence reads. Local CLI `node .agents/bin/agent-notes.js`: new --kind <kind> --topic <topic>
--id <id> "title"; CLI show/write use its read_id.
id and kind are stable; class applies only to decisions. Link details and declare assets with
one owner. Correct facts in place; new choices declare supersedes with all or explicit owns scope.

Rejected decisions and archives freeze content and assets: never edit, move or delete them.
Cancel or withdraw unfinished work before archiving. Finish with `node .agents/bin/agent-notes.js verify` and inspect
the diff. Verification does not prove conclusions; queued indexing is not completed indexing.
