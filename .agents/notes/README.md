# Agent Notes: repository knowledge

Canonical content lives under topics/<topic> with agent-note/2 metadata. Legacy decisions keep
paths and seals. Choose decision, plan, research, analysis, design or guide. A small topic needs
only one Note. Update the existing owner; do not create a duplicate ADR to satisfy documentation.
Plans own tasks/dependencies; designs own contracts; decisions own rationale; research/analysis
own scoped evidence and findings; guides own operations. Link details instead of copying them.
Declare HTML, images and evidence excerpts as assets; keep executable code in one location.

Use `node .agents/bin/agent-notes.js new --kind research --topic playback --id player-research "Player investigation"`.
Read show for content and SHA-256, then write with --content-file and --expected-sha256.
Use transition for state changes, retaining paths. Frontmatter accepts top-level fields and JSON
flow arrays/objects, without YAML tags, aliases or nested blocks. id and kind remain stable.

States: decision proposed/implemented/rejected; plan planned/active/completed/cancelled;
research/analysis draft/completed/cancelled; design draft/accepted/withdrawn;
guide draft/current/obsolete/withdrawn. Completed research and accepted designs do not claim
shipped behavior. New decisions/contracts supersede rather than rewrite earlier choices.
Supersedes edges require scope: all or explicit owns keys. Optional owns keys have one effective
owner; semantic duplication still requires reading. Archives seal content and assets in place;
legacy seals stay intact. Rejected decisions are frozen. Archive only settled content.

inventory lists ungoverned Markdown in configured document_roots (default docs).
migrate <plan.json> --dry-run checks reviewed input digests and explicit outputs; --apply upgrades
configuration in the same transaction. Never downgrade config to simulate rollback. recover handles
interrupted transactions; investigate a writer lock before recovery and preserve concurrent edits.
Finish with `node .agents/bin/agent-notes.js verify`. Mechanical verification cannot prove findings or semantic uniqueness.
CE type=note searches all kinds; type=decision keeps its existing scope. Read evidence before use.
Queued indexing is not completed sync/embedding.
