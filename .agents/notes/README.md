# Agent Notes

An **Agent Note** records a decision or proposal that affects this codebase — the *why* and *what we gave up*, the parts code and docs can't carry. They are effectively RFCs / decision records written by agents (and humans) in the same change as the code. This file defines where they live, when to write one, and the in-file format. `node .agents/bin/agent-notes.js verify` enforces everything mechanical below.

`verify` ignores macOS `.DS_Store` metadata at the notes root, lifecycle, and class levels; other unexpected files still fail validation.

## Layout and naming

Every note has two axes, both encoded in its **path** — `{lifecycle}/{class}/yyyy-mm-dd-topic-slug.md`:

- **Lifecycle** (top-level folder) is the note's status; a note moves between folders as that changes:
  - **`proposed/`** — reviewed before implementation; not built yet (or only partly).
  - **`implemented/`** — the decision shipped. The note is **kept current with what shipped**: when code later moves a file, renames a module, or changes a default, update the note in the same change (facts only — see [implemented/AGENTS.md](implemented/AGENTS.md)).
  - **`rejected/`** — considered and declined. Keep it only while its rationale prevents a tempting, meaningful mistake; otherwise delete it.
  - **`archived/`** — frozen history, see [Archiving](#archiving-and-deletion).
- **Class** (nested folder) is the *kind* of decision — see [Classification](#classification).

The date is when the topic was **first proposed**. Cross-references between notes are relative markdown links, never bare prose, so they are checkable and survive moves (`move` and `archive` repair inbound links). There is no index file: browse the folders, `node .agents/bin/agent-notes.js list`, or search.

## Classification

The class set is closed and lives in `config.json`; `verify` rejects other folders. Adding a class means updating `config.json` and this table.

| Class | What it covers |
|---|---|
| `feature` | A new user- or model-facing capability. |
| `bug-fix` | Corrects a defect or closes a gap a postmortem surfaced. |
| `simplification` | Removes code, behavior or surface area without adding a capability. |
| `architecture` | A structural decision about the shipped source — how modules relate, what the runtime vocabulary is. |
| `process` | Tooling, policy or workflow around the code — gates, package manager, CI — not runtime behavior. |
| `testing` | Test infrastructure and strategy. |

`architecture` is about the source we ship; `process` is the tooling and workflow around it. There is deliberately no `refactor`: "does observable behavior change?" already separates `simplification` from the rest.

## When to write one

**Every non-trivial change MUST add or update at least one Agent Note in the same change.** Non-trivial means it alters behavior, architecture, a contract shared across files or modules, process or tooling, testing strategy, an on-disk / wire / configuration format, or another decision a maintainer may reasonably revisit. Only purely mechanical or local edits are exempt.

- Substantial future work starts in `proposed/`; a decision already made starts in `implemented/`.
- **Check for supersession first:** `node .agents/bin/agent-notes.js related <keywords>`. Updating the note that already owns a decision satisfies the rule — do not create a duplicate.
- A note is never edited into a *different decision*. Reverse or replace a decision with a new note, and cross-link the two. Editing an implemented note to track where its existing decision now lives is required, not forbidden.
- A fully superseded implemented note may be folded into its successor and deleted, but only after the successor preserves every unique rationale, alternative, consequence and required verification, and every inbound link is repaired. Partial supersession keeps both notes, cross-linked.

## The file format

### Header

The first lines of every note are exactly:

```markdown
# Agent Note: <title>

Status: <status>

```

`Status:` is `proposed`, `implemented`, or `rejected — <why, in one line>`, and must agree with the folder. No dates, no parentheticals: the filename holds the date and git holds the rest. The rejection reason is the one status with content, because that verdict is what readers come for.

### Body skeleton

Every note opens with `## Problem` — the motivation, written to stand without the solution. Recurring sections use these canonical names (the zh equivalents `问题 / 提案 / 决策 / 备选方案 / 验收标准 / 风险 / 影响` are accepted too); bespoke technical sections (schemas, wire contracts, topology) go freely between them.

| lifecycle | required sections, in order |
|---|---|
| `proposed/` | Problem · Proposal · Alternatives considered · Acceptance criteria · Risks |
| `implemented/` | Problem · Decision · Alternatives considered · Consequences |
| `rejected/` | Problem · Proposal · Alternatives considered (the proposal, frozen) |

- **Proposal** may speak in the future tense; plans, migration steps and open questions belong here while the work is unbuilt. **Acceptance criteria** is the observable state that means done. **Risks** covers what could go wrong *and* what the change knowingly gives up.
- **Decision** describes shipped reality in the present tense. **Consequences** records what the trade-off cost *and* bought, and what now pins the behavior. Proposal-era headings (`Proposal`, `Plan`, `Migration plan`, `Acceptance criteria`) are rejected in an implemented note. `## Testing` or `## Related` are fine where they state present-tense fact.

### Alternatives considered — mandatory

Each genuine alternative and why it lost: one bold-led paragraph per alternative, or a `### Why not <X>?` subsection per contested one. A decision recorded without what it beat invites re-litigation — the failure Agent Notes exist to prevent. Alternatives are **recorded, never invented**; a note imported from before this format whose alternatives cannot be reconstructed carries `<!-- agent-note-format: alternatives-not-recorded -->` instead.

### Moving between lifecycles

`node .agents/bin/agent-notes.js move <note> implemented` / `move <note> rejected --reason "..."` updates the Status line, moves the file and repairs links. Moving to `implemented/` then requires rewriting it for shipped reality in the same change: Proposal → present-tense Decision; Acceptance criteria and Risks → Consequences (or a present-tense `## Testing`); plans dropped in favor of what shipped. `verify` fails until that is done. Moving to `rejected/` only adds the reason and freezes the file.

## Archiving and deletion

Archive an implemented note when the shipped decision is complete and its rationale is unlikely to guide future work (one-off UI detail, a narrow adapter, a minor closed bug, superseded implementation detail). Keep it active while its alternatives, ownership boundary, negative guarantee, durable or wire semantics, security rule, or reintroduction condition remains useful — judge by future decision value, never by length or age.

- Never archive a proposed note: reject an obsolete proposal instead.
- Keep a rejected note only while it prevents a plausible mistake; otherwise delete it and repair inbound links.
- `node .agents/bin/agent-notes.js archive <note>` moves it to `archived/{class}/`, inserts `Archived: YYYY-MM-DD` below `Status: implemented` (the only content change allowed), seals its hash in `archived/manifest.json`, and retargets inbound links — then redirect each to current authority or delete it, unless it cites history on purpose.

Once sealed, an archived note is frozen: never edit, reformat, move or delete it, and never treat it as authority for current behavior. `verify` rejects modified or missing sealed notes and does not check their outbound links.
