# Design: one way to reconcile a failed merge

**Status:** design for aegis-k7l (2026-10-28). Builds on shipped work; it does
not replace it.

Steve's ask: agents need one way to reconcile a merge that could not finish,
need to know how to do it, and need a standard way to show it to the user,
including in the Git merge tool.

## 1. What exists

Quipu has three merges that can stop on a conflict. All three already use the
same operator (`src/merge3.rs`) and the same conflict record
(`merge3::DecisionRecord`: subject, predicate, `sh:maxCount`, base / ours /
theirs values). A conflicting slot is held at its base value and nothing is
guessed.

| Merge | Detect | Resolve today | Shipped in |
|---|---|---|---|
| Share reconnect, `quipu merge <share-dir>` | exit 2, conflicts in the JSON result | **emit / propose / decide / apply**: `--emit-decisions <file> [--propose]`, then `--decisions <file> --reviewer <who> [--dry-run]`; REST `/merge/decisions`, `/merge/apply`; MCP `quipu_merge_decisions`, `quipu_merge_apply`. Rows carry each side's provenance, a `kind`, a `rule` in words, labels, a mechanical `proposal`, and the operator's `decision`. v2 files bind each reviewed slot (`local_slot_hash`). | #435, #658 |
| Git pendant merge, `quipu git-merge <ref>` / `merge-driver` | exit 2, `decisions.json` beside the qpack | `quipu pendant-resolve BASE OURS THEIRS DIR KEY CHOICE` (base/ours/theirs, accept/reject for alias proposals); CI `pendant-check` refuses an unrecorded or text-merged result | #346 |
| PR review of a qpack change | `quipu share diff --report --format markdown` | read-only "Merge decisions" section in the check-run summary (labels, constraint, base/ours/theirs, resolution, alias caveat) | fxpbys.1 M1/M2 |
| Fork promote, `quipu fork promote <name>` | `ForkPromotion::Conflicts`, nothing written, fork stays open, exit 1 | **none** | #888 |

## 2. The gaps

1. Fork promote has no resolve or apply step.
2. The three resolve paths take different inputs (a JSON `decision` object,
   a `CHOICE` string with three Git refs) and none of them presents a
   conflict the same way twice.
3. There is no standard user-facing layout. The PR-review markdown is close,
   but it is a report, not a question to the user, and it has no choices.
4. No agent guidance: nothing in the primer, the skill or the book says when
   an agent may decide a conflict on its own and when it must ask.

## 3. The design

### 3.1 One record: the decisions-file row

The share decisions row (`share_merge_decisions::DecisionRow`) is the shared
record. It already holds everything the item asks for: the slot
(`DecisionRecord`), each side's provenance, `kind` and `rule`, `proposal` and
`decision`, `decided_by` / `decided_at`.

- **Fork promote** emits the same row shape in a fork decisions file
  (schema `merge-decisions/fork-v1`), bound to the fork name, its base
  transaction, and a slot hash per row on BOTH sides (ROOT and the fork),
  computed exactly as the share v2 `local_slot_hash`. Its provenance lists the
  current facts on each side (`valid_from`, tx, actor, source).
- **Git** keeps its `decisions.json` schema, because `pendant-check`
  compares it byte-for-byte with a recomputation in CI. It is read into the
  same presentation (3.3) and resolved through the same verb (3.2).

Choices, on every surface:

| Choice | Result | Offered when |
|---|---|---|
| `ours` | the local side's values (ROOT, or Git `HEAD`) | always |
| `theirs` | the incoming side's values (share, fork, or the merged branch) | always |
| `base` | the value both sides started from | always |
| `both` | ours ∪ theirs | only when that fits the slot's `sh:maxCount` |
| `value` | values the user typed, as N-Triples terms | share and fork (Git: not yet) |
| `defer` | leave the row undecided; apply refuses until it is decided | always |

### 3.2 Verbs per surface

The decisions file is the merge id: one file per failed merge, on disk for
the CLI and inline for MCP.

| Step | Share | Fork promote (new) | Git pendant |
|---|---|---|---|
| emit | `quipu merge <dir> --emit-decisions f.json [--propose]` | `quipu fork promote <name> --emit-decisions f.json [--propose]` | written by `quipu git-merge` as `<qpack>/decisions.json` |
| show (new) | `quipu merge show f.json [--format text\|md\|json]` | same | `quipu merge show <qpack>/decisions.json` |
| resolve (new) | `quipu merge resolve f.json <row> --choose <c> --by <who>` | same | same, or `quipu git-merge resolve <qpack> <key>`; it rewrites the qpack (refs from `MERGE_HEAD`); `pendant-resolve` stays for scripts |
| apply | `quipu merge <dir> --decisions f.json --reviewer <who> [--dry-run]` | `quipu fork promote <name> --decisions f.json --reviewer <who> [--dry-run]` | `git add <qpack>` and commit; CI `pendant-check` |

MCP: `quipu_merge_decisions` and `quipu_merge_apply` take `fork: <name>` as
an alternative to the inline shares, and `quipu_merge_decisions` returns the
standard markdown in `presentation`. The new `quipu_merge_resolve` takes a
decisions file, a row and a choice, validates the choice against the row, and
returns the updated file plus `presentation`; it writes nothing to the store.

Apply rules, unchanged from #435 and extended to forks:

- all-or-nothing, in one transaction whose source names the reviewer and the
  SHA-256 of the decisions file;
- refuses on an undecided or deferred row, a stale file (either side of a
  reviewed slot moved, or the conflict set changed), too many values, or a
  value that is not an RDF term;
- fork apply re-enters through the same SHACL and write gates as a clean
  promote; a SHACL refusal writes nothing;
- **idempotent**: applying a file whose hash is already recorded on a
  transaction returns that transaction with `outcome: "already-applied"` and
  writes nothing.

### 3.3 The standard presentation

One layout, three renderings of the same data. Agents show the user exactly
this and do not paraphrase it.

Text (`quipu merge show f.json`, the default):

```text
Merge needs your decision: fork 'triage' -> ROOT
1 conflict, 1 undecided. Nothing has been written.

[d1] Ticket 42 · status
  rule     sh:maxCount 1 on status: ours and theirs together hold 2 values
  base     "open"
  ours     "blocked"  (alice, 2026-10-27T14:02:00Z, tx 118)
  theirs   "closed"  (triage-bot, 2026-10-27T15:40:00Z, tx 131)
  suggest  theirs: theirs is newer: fork tx 131 >= ROOT tx 118
  choose   1) ours  2) theirs  3) base  4) value  5) defer
  decided  (none)

Next: quipu merge resolve f.json d1 --choose <ours|theirs|base|value|defer> --by <you>
Then: quipu fork promote triage --decisions f.json --reviewer <you> (add --dry-run first)
```

Markdown (`--format md`, MCP `presentation`, for chat):

```markdown
**Merge needs your decision:** fork 'triage' -> ROOT
1 conflict, 1 undecided. Nothing has been written.

#### d1. Ticket 42 · status

| | Value | Changed by |
|---|---|---|
| base | `"open"` | |
| ours | `"blocked"` | alice, 2026-10-27T14:02:00Z, tx 118 |
| theirs | `"closed"` | triage-bot, 2026-10-27T15:40:00Z, tx 131 |

Rule: sh:maxCount 1 on status: ours and theirs together hold 2 values
Suggested: **theirs**: theirs is newer: fork tx 131 >= ROOT tx 118
Decided: (none)

1. ours
2. theirs
3. base
4. value
5. defer

Next: `quipu merge resolve f.json d1 --choose <ours|theirs|base|value|defer> --by <you>`
Then: `quipu fork promote triage --decisions f.json --reviewer <you> (add --dry-run first)`
```

JSON (`--format json`): schema `merge-view/v1`, one object per conflict with
`id`, `entity` {iri, label}, `field` {iri, label}, `rule` {kind, max_count,
text}, `base`, `ours`, `theirs` ({values, changed_by}), `suggestion`,
`choices`, `decision`, `decided_by`, `decided_at`; plus `merge` {kind,
title}, `undecided`, `next`.

These two blocks are the snapshots `src/merge_reconcile_tests.rs` pins.
Labels come from `rdfs:label` (the file's `labels` map); an IRI without one is
shown in full. Values are N-Triples terms, so a literal keeps its quotes. The
last lines always name the exact next command.

### 3.4 Agent rules

1. On a failed merge, emit decisions with `--propose`, then show the user the
   standard layout. Do not summarize it away.
2. Decide a row yourself only when **both** sides of that slot were written by
   agents (every `changed_by` is an agent identity) and you take the
   `suggest`ed choice. Record it with `--by <your-agent-id>` and say so in one
   line.
3. Ask the user, never auto-decide, when any side was written by a person,
   when you cannot tell who wrote a side, for `value` and `both`, and for
   `sh:maxCount` conflicts on human-owned data. A class-B predicate (money,
   legal, irreversible, credentials, direction) waits for a signed human
   verdict (aegis-kzt0ql.9) once that ships.
4. Ask once, for all undecided rows together, with the numbered choices.
   Record each answer with `--by <the user>`.
5. Apply with `--dry-run` first, then for real. Verify with `quipu merge show`
   (0 undecided) and the apply outcome (`merged` / `promoted`). Re-applying
   the same file is a no-op that reports `already-applied`.
6. If apply says the file is stale, emit again; never edit hashes.

## 4. Delivery

1. **Shared record, show/resolve, fork.** `src/merge_reconcile.rs`
   (presentation + choice validation), `quipu merge show/resolve`,
   `quipu_merge_resolve`, `presentation` on `quipu_merge_decisions`,
   `both`, idempotent apply, fork emit/propose/apply on CLI and MCP.
2. **Git.** `quipu merge show/resolve` on `decisions.json` (refs from
   `MERGE_HEAD`), `both` in the Git applier, the driver and `git-merge`
   print the standard layout and the exact next command.
3. **Guidance.** A primer section, a skill recipe and a book page,
   "Reconciling a failed merge", covered by the docs drift gate.

Tests: for each merge kind, conflict -> show -> resolve each choice ->
apply -> SHACL passes -> re-apply is a no-op; snapshot tests of the three
renderings.
