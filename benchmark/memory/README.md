# ARM F: Memory metadata on real Dolt

Compare **native Dolt JSON**, a **whole-cell TEXT ablation**, and the proposed
typed triple rows. This is synthetic research, not a tracker migration.
Read [RESULTS.md](RESULTS.md) before quoting numbers.

```bash
just bench memory --selftest --scratch /tmp/memory-fixtures --output /tmp/memory-controls
just bench memory --seeds 1,7,42,99,2026 --scratch /tmp/memory-fixtures --output /tmp/memory-results
just bench memory-report /tmp/memory-results
```

Requires Python 3.12+, uv and Dolt 2.1.8. `uv.lock` pins the MySQL client. Each
seed gets a new directory; occupied ports and existing directories refuse.
The SQL-server child binds only loopback, port 13318, and is terminated and
waited for in `finally`, including failed runs. Logs and runtime identity remain
in the scratch directory. No systemd unit, timer, remote or production database
is used. Change `--port` only to another unused local fixture port.

## Storage and source identity

`beads-issues.sql` is the exact `0001_create_issues.up.sql` migration from
`gastownhall/beads` commit `e1ceedf095d20569ab69adfdb7233f214e0a5ffa`, retrieved
through GitHub's contents API. It retains the `issues.metadata JSON` column.
The copied file is MIT-licensed under that repository's licence. This is the
initial issues-table migration, not a claim that all later Beads migrations
were applied. Unrelated fields are fixed; created/updated timestamps are pinned
so clock-generated writes cannot contaminate the metadata comparison. The
unchanged Memory body occupies the `description` cell.

The indexed alternative stores `(bead_id, key, slot, value, datatype,
set_in_version)` with primary key `(bead_id,key,slot)` and index `(key,value)`.
Single-valued keys use `slot=''`; many-valued keys use `slot=value`. Unchanged
facts retain `base` provenance; new facts carry `left` or `right`. This deliberately
tests provenance as part of the row. Identical new values from both versions can
therefore conflict on provenance; no hidden deduplication suppresses that result.
These version labels refer to the published per-Memory input snapshots, not
self-referential Dolt commit hashes.

Native JSON is a stronger baseline than a cell-only model. Dolt documents
[per-key JSON object merging](https://www.dolthub.com/docs/concepts/dolt/git/conflicts/).
Concurrent arrays are also tested, with the actual installed engine deciding
whether they conflict. TEXT is labelled as an ablation throughout.

## Competency questions define the declaration

[competency/memory.md](competency/memory.md) follows the numbered Markdown suite
format accepted by Camayoc's `scripts/competency.py --suite <directory> --list`.
The table below is a declared policy justified by those questions, not an
empirically discovered property of all Memories.

| key | cardinality | datatype | competency question |
| --- | --- | --- | --- |
| status | at most one | string | What is the current status of this Memory? |
| valid-until | at most one | timezone-qualified dateTime | Until when is this version valid? |
| evidence | many | string | Which sources support this Memory? |
| applies-to | many | string | Which targets does this Memory apply to? |
| tags | many | string | Which categories include this Memory? |

The prototype writer validates the complete candidate before deleting/inserting
rows. Five invalid candidates must be refused with the existing row snapshot
unchanged. The same candidates must actually persist through native JSON SQL,
then be restored. This demonstrates a declaration guard, not native Dolt SHACL
enforcement; a JSON writer can apply the same guard.

## Instruments and accounting

Ten explicit templates, twenty copies each per seed: independent scalar keys,
divergent scalar, identical scalar, distinct set additions, identical set
addition, set remove/add, scalar delete/edit, independent set keys, mixed genuine
and harmless edits, and unchanged. Seeds vary labels/order, not template counts.
The generator publishes the three input versions and an explicit expected
accepted version; controls independently recover true scalar conflicts from
the three versions. Balanced counts are not five independent estimates of
production conflict frequency.

`CALL DOLT_MERGE` and `dolt_conflicts_<table>` supply decisions; there is no
reimplementation of Dolt. Native conflict rows are reported separately from
semantic-slot TP/FP/FN. A conflicted JSON/TEXT Memory expands to its changed
metadata keys for semantic-slot accounting; this does not mean Dolt prompts once
per key. A genuine status conflict plus harmless tag changes consequently has
one native decision and collateral false slot questions.
An entirely false native decision contains no oracle conflict. That count is
reported separately: fewer collateral slots need not mean fewer native decisions.

All conflicted units are held at their base values when scoring automatic
output, matching Arm A's accounting convention. Lost/extra facts include
deferred valid changes and base facts kept while a decision is pending; they
are not claims of irreversible deletion. Scoring controls force a missed
conflict, a false conflict, one missing fact and one fabricated fact.

The query trial restores a common, unmerged dataset: 2,000 Memories, 10,000
metadata facts and 2,000 Links per seed. Status equality, tag membership and a
metadata-to-Link join must return exactly the independently generated answers
in **every** repetition. Coverage and nonempty positive answers are required.
Each query warms once and has seven timed executions; times include the local
MySQL round trip and result transfer. JSON runs before rows within each query;
this is not a randomized performance trial. Full samples and `EXPLAIN` plans
are published, including slow indexed joins. Dolt 2.1.8's `EXPLAIN` emits textual
`NULL` in numeric estimate columns; the client decodes that only for `EXPLAIN`.

## Evidence and limits

`results/input-<seed>.json` publishes all merge inputs and expected outputs;
`results/metrics.json` publishes counts, conflict identities, SQL, plans, samples,
coverage, type controls and fixture-stop evidence. Runtime paths/PIDs remain
private; the renderer strips them rather than copying them into public files.
The source file hashes bind the implementation used for each run.
`just bench memory-report` regenerates RESULTS.md and both LaTeX tables directly
from saved measurements. Timings can change on reproduction; conflict counts
and SQL answers must not.

This arm does not exercise body editing, production workloads, alias resolution,
blank-node canonicalization, automatic SHACL derivation or divergent shapes.
The earlier 0/93 alias blindness, negative Arm D and single-deployment limitation
remain. Array order is storage syntax; the oracle declares these arrays to be
sets. Other applications may declare ordered arrays instead and need a
different oracle.
