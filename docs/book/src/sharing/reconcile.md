# Reconciling a failed merge

Three Quipu merges can stop on a conflict:

| Merge | Command | How it stops |
|---|---|---|
| Share reconnect | `quipu merge <share-dir>` | exit 2, conflicts in the JSON result |
| Fork promotion | `quipu fork promote <name>` | "promotion held", exit 1, the fork stays open |
| Git pendant merge | `quipu git-merge <ref>` | exit 2, `<qpack>/decisions.json` beside the pack |

All three use the same shape-aware operator. A slot (one subject and
predicate) that both sides changed differently, past an `sh:maxCount` bound,
is held at its base value. Nothing is guessed and nothing is written. This
page is the one way to finish such a merge, for a person or an agent.

## The four steps

| Step | Share | Fork | Git |
|---|---|---|---|
| 1. Emit | `quipu merge <dir> --emit-decisions f.json --propose` | `quipu fork promote <name> --emit-decisions f.json --propose` | written by `quipu git-merge` |
| 2. Show | `quipu merge show f.json` | same | `quipu merge show <qpack>/decisions.json` |
| 3. Resolve | `quipu merge resolve f.json <row> --choose <choice> --by <who>` | same | same, or `quipu git-merge resolve <qpack> <key> ...` |
| 4. Apply | `quipu merge <dir> --decisions f.json --reviewer <who>` | `quipu fork promote <name> --decisions f.json --reviewer <who>` | `git add <qpack>` and commit; CI `pendant-check` verifies |

Over MCP, step 1 is `quipu_merge_decisions` (`fork: <name>` for a fork,
`present: true` for the layout), step 3 is `quipu_merge_resolve`, and step 4
is `quipu_merge_apply`. `quipu_merge_resolve` touches no store; it returns the
updated decisions for you to pass to apply.

Run apply with `--dry-run` (`dry_run: true`) first. It makes every check,
including SHACL for a fork, and writes nothing.

## Choices

| Choice | Result |
|---|---|
| `ours` | the local side: ROOT, or Git `HEAD` |
| `theirs` | the incoming side: the share, the fork, or `MERGE_HEAD` |
| `base` | the value both sides started from |
| `both` | ours and theirs together; offered only when that fits `sh:maxCount` |
| `value` | values you type, as N-Triples terms (`--value '"wontfix"'`); share and fork only |
| `defer` | leave the row undecided; apply keeps refusing until it is decided |

Git alias proposals (two new IRIs that may name one entity) take `accept`
(adds `owl:sameAs`), `reject` or `defer`.

## What the user sees

`quipu merge show` prints one fixed layout. Agents show it as printed, with
`--format md` in chat:

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

Each block names the entity and the field by `rdfs:label`, the rule that made
it a conflict, base, ours and theirs with who changed each and when, the
mechanical suggestion with its reason, and the numbered choices. The last
lines always give the exact next command. `--format json` gives the same data
as `merge-view/v1` for tools.

## When an agent may decide

- **Decide alone** only when every side of the row was written by agents and
  you take the suggested choice. Record your agent id with `--by` and tell the
  user in one line.
- **Ask** when a person wrote either side, when you cannot tell who did, for
  `value` and `both`, and for every `sh:maxCount` conflict on human-owned
  data. Ask once, for all undecided rows, with the numbered choices, and
  record each answer with `--by <the person>`.
- High-stakes predicates (money, legal, credentials, irreversible changes)
  will need a signed human verdict once hardware-backed verdicts ship.

## Refusals

Apply writes nothing and says why when a row is undecided or deferred, when
the decisions are stale (a reviewed slot moved on either side, or the set of
conflicts changed), when a decision has more values than `sh:maxCount`, when a
value is not one RDF term, when the file has a field its schema does not
define, or, for a fork, when SHACL refuses the result. A stale file is emitted
again and re-decided; never edit its hashes.

Applying the same decisions a second time reports `already-applied` with the
original transaction and writes nothing, so a retried apply is safe.

## See also

- [CLI: Sharing & Legacy Packs](../reference/cli-sharing.md): `quipu merge`,
  the decisions file fields, and the Git pendant commands.
- [MCP tools](../reference/mcp-tools.md): `quipu_merge_decisions`,
  `quipu_merge_resolve`, `quipu_merge_apply`.
- Design: `docs/design/merge-reconcile.md`.
