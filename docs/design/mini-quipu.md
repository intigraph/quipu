# Mini-Quipu: per-component store lifecycle

Status: proposed design, slice 1 of the mini-Quipu directive. This note grants
no runtime, configuration, deployment or migration authority. It creates no
store and changes no write path.

Directive (Stiwi, 2026-11-21, verbatim): "Mini-kipu is the ability to use
individual kipu databases for each part of the kipu stack. So each agent could
potentially get their own seeds database and then merge back later."

Quipu stays generic. Seeds appears below only as a worked example. Nothing in
this design names an application, and no rule is written for one.

This note builds on four existing designs and reuses their names:

- [Per-graph writable stores](per-graph-writable-stores.md): catalog,
  router, physical owner, transaction coordinate, composition cut, strict
  snapshot.
- The Phase 1 graph-set source in quipu#1310 and #1311: `GraphSetCatalog`,
  `GraphSetOwner`, `GraphSetRouter`, `GraphSetDataset` and the default-off
  `[quipu.graph_sets]` startup table.
- [Merge behaviour](merge-behaviour.md) (the karb oracle) and the one write
  core in `src/merge_core.rs` (`plan_merge`, merge id, `receive`/`decide`).
- [Fork-at-any-event](fork-at-any-event.md) and fork promote as a three-way
  merge (`src/store/forks_merge.rs`).

## 1. What a mini-Quipu is

A mini-Quipu is **a fork whose facts live in a separate store file**. It holds
a declared graph set, copied from the home store at one cut. One component or
one agent owns its single writer.

| Property | Value |
|---|---|
| Storage | One SQLite store file with its own `store_id`, `log_epoch`, transaction log and writer |
| Content | A declared graph set: exact graph IRIs, each a **writable member** or a **read-only reference member** |
| Base | The home store's state of those graphs at one recorded cut, plus a base digest |
| Owner | One principal: a component or an agent. It is the only writer of the file |
| Home record | A fork registry entry in the home store that names the file identity, base cut, digest, graph set and pinned shapes revision |
| Way back | Fork promote (U2) through `merge_core`. There is no other way |

What it is **not**:

- It is not a routed owner. A graph that the home catalog routes to a writer
  keeps that writer. The mini-Quipu holds a fork of that graph's state, not a
  second writer for its IRI (refusal X1).
- It is not a replica. A replica replays accepted home commits. A mini-Quipu
  holds proposals that home has not admitted yet. The two paths do not share
  an entry point, as [local sync mode](local-sync-mode.md) §7 already
  requires.
- It is not the n0wc.5 Seeds owner file. That file is a routed owner: one
  writer, live composition, no copy and no merge back. A mini-Quipu forked
  from it is an optional extra for one agent, and it leaves the routed file
  unchanged.

Graph and entity IRIs stay the same in the mini-Quipu. Terms are file-local
allocations, as in every routed file. Identity across files is by IRI and
literal value, never by term ID (per-graph design, "Terms, links and query
semantics").

## 2. Lifecycle

```text
 home store                         mini-Quipu file
 ──────────                         ───────────────
 strict snapshot of the set ──fork──▶ fresh store_id + log_epoch
 fork registry entry (base cut,      members copied at the cut
   digest, set, shapes revision)     │
          │                          ▼
          │                        local writes (ordinary write pipeline)
          │                          │
          ◀───────── merge back = fork promote (U2) via merge_core
          │                          │
 receive / decide txs                ▼
 (one owner savepoint)             retire: fenced read-only, registry closed
```

### 2.1 Fork: which graphs, at which cut

1. **Declare the graph set.** The fork names exact graph IRIs. Each one is a
   writable member or a read-only reference member (for example an ontology or
   shapes graph that local validation needs). The set follows the same rules as
   a v1 graph-set catalog file: exact IRIs only, with no prefix or pattern
   field (B2). If a writable member belongs to a `GraphSetCatalog` set, the
   whole set must be writable members of the fork (B1, B5).
2. **Resolve against home.** Authorize the set against the home catalog
   generation and the caller's grants. Every writable member must be on one
   physical owner at home: one routed `GraphSetOwner`, or the legacy file.
   A set that spans home files refuses (X4) until Phase 3 participant commit
   exists, because the per-graph design refuses multi-file fork bases
   ("Identity, distribution, backup and rollback").
3. **Capture the cut.** Take a strict snapshot of that owner. Because there is
   one owner, the cut is one transaction coordinate
   `(store_id, log_epoch, local_tx)` plus the catalog generation and policy
   revision. Read-only reference members on another owner join the same strict
   capture as the per-graph design describes. They are copied, never written.
4. **Materialize.** Create a fresh file and mint a new `store_id` and
   `log_epoch` for it. Copying the home file does not create a valid store,
   because `store_id` survives a file copy
   ([local sync mode](local-sync-mode.md) §2). Copy every member as of the
   cut, with valid time, in the way `fork_create` copies a same-store fork
   today. Pin the home shapes and policy revision that apply at the cut.
5. **Record at home.** Write one fork registry entry: the fork name, the
   mini-Quipu `store_id` and `log_epoch`, the base cut, the graph set, the
   pinned revisions and the base digest. The digest follows the local-sync
   `base_slice_hash` construction: each canonical N-Quad line paired with its
   valid interval, sorted, then hashed. The entry is a control write on the
   owner that holds the members. It is not a catalog route.

### 2.2 Local writes

The mini-Quipu runs Quipu's ordinary write pipeline on its own file: authority,
ownership fence, SHACL under the pinned shapes, policy, conditional writes and
graph batches. Its writes are visible only in the mini-Quipu. Home reads,
home composed reads and `GRAPH ?g` at home never include mini-Quipu state.

Rules:

- Writes to read-only reference members refuse (X6).
- Derived facts (reasoner output) go to a derived graph that is never merged,
  as local-sync §7 specifies for overlays.
- A conditional write or compare-and-set is checked against the mini-Quipu
  only. It is not a home precondition. At merge time it becomes an ordinary
  slot change that R1-R8 judge (see the example in §5).
- Cursors, `as_of_tx` and `--at` values from the mini-Quipu name its
  `store_id` and `log_epoch`. Home refuses them, and the mini-Quipu refuses home
  cursors (B4, X8).

### 2.3 Merge back: fork promote through the one core

Merge back **is** fork promote, use case U2 of the oracle. The only change is
where the `theirs` facts are read from. It adds no fourth write path:

| Side | Same-store fork promote today | Mini-Quipu merge back |
|---|---|---|
| base | ROOT as of `fork_tx` | home owner as of the recorded cut coordinate, checked against the base digest |
| ours | ROOT now | the same home owner now, scoped to the declared writable members |
| theirs | the fork now | the mini-Quipu file now, read through a read-only connection |
| value rule | `merge3::merge_slots` | unchanged |
| plan | `merge_core::plan_merge` | unchanged |
| write | `receive` then `decide`, one savepoint (§3.2) | unchanged, in the home owner's one savepoint |
| merge id | oracle §2: path `fork`, incoming `fork name@fork_tx` (at this revision only the share path calls `merge_core::merge_id`) | the same function and the `fork` path. The incoming identity names the mini-Quipu `store_id`, `log_epoch` and head `local_tx` |

The oracle's rule R1 allows paths to differ only in their inputs and output
target. A mini-Quipu provides exactly the fork inputs of §3.1: each fact's own
`valid_from`, the close rows of removed values and per-fact provenance. Its
output target is the store. So this is the existing fork path, and it inherits
the U2 fixtures. Slice M3 (§7) proves this: the oracle cases give the same
results for a same-store fork and for a mini-Quipu with identical facts.

Admission, in order. Any failure writes nothing and leaves the mini-Quipu
unchanged:

1. The registry entry is open. The mini-Quipu `store_id` and `log_epoch` match
   it. The home owner's `store_id` and `log_epoch` match the base cut
   (`foreign-epoch` otherwise).
2. Home can rebuild the base as of the cut (`base-unavailable` otherwise, for
   example after a deep freeze). The rebuilt base matches the digest
   (`base-mismatch` otherwise).
3. The applier has **current** write authority on every writable member at
   home. Authority recorded in the mini-Quipu is not trusted.
4. All writable members are on one home owner under the current catalog
   generation. If a member moved since the fork, the merge refuses (X4).
5. `plan_merge` runs. `receive` and `decide` pass the home write gate under
   the **current** home shapes, without an exemption. Shapes the mini-Quipu
   pinned are not trusted at home.
6. Partial modes (`atomic` by default, `partial-entity`, `partial-slot`) and
   merge units follow §6 of the oracle. A held slot is not received.

Retry needs no new journal (B6). Merging the same input again with the same
decisions reports `already-applied` with the original transaction IDs (R7). If
a reply is lost, the caller reads back by merge id, which is the existing
read-back UNKNOWN contract, before it tries again.

Several merges from one mini-Quipu are allowed. After a merge, the registry
entry's base advances to that merge's home `decide` (or `receive`)
coordinate, paired with the mini-Quipu head that was merged. The next merge is
then three-way against that new base. This is the same rule as a later apply
of a partial remainder (oracle §6): a new merge id that names the earlier one.

### 2.4 Retire

Retirement happens after the final merge, or when the owner abandons the
fork:

1. Fence the mini-Quipu writer, so the file becomes read-only.
2. Mark the home registry entry retired, recording the last merge id or
   "abandoned". Retirement writes nothing to home member graphs.
3. Keep the file until its merge receipts are verified at home. Deleting the
   file afterwards is an operator choice. Home history never depends on the
   file, because merged facts and their provenance are in home transactions.

A retired fork never reopens. A new fork is a new mini-Quipu with a new
identity.

## 3. How it sits on per-graph writable stores

| Boundary | Effect on a mini-Quipu |
|---|---|
| Catalog and router (Phase 1, quipu#1310/#1311) | The mini-Quipu is never a catalog route and never enters `[quipu.graph_sets]`. It reads from routed or legacy owners only through the normal snapshot path. Its merge back writes through the one home owner that the router already admits for the members. All-routed batches go to that owner. A merge that would span files refuses, as `GraphSetRouter` already does for mixed batches |
| B1: a graph set is atomic | A fork must take every member of a catalog set, or none. The Seeds example cannot fork the project graph without its ephemeral, provenance and archive graphs, the ops-log family and the identity family |
| B2: open-ended families stay legacy | The mini-Quipu graph set uses exact IRIs only. A family such as weekly message windows cannot be forked until aegis-rkhn defines a membership rule. A fork that names one window as a single exact IRI is just a legacy-graph fork, and it carries no family semantics |
| B3: store-wide discovery | Inside a mini-Quipu, `GRAPH ?g` and the default graph range over its own members only. A local check that needs other graphs must name them as read-only reference members, or it is incomplete (UNKNOWN), not empty. At home, merge admission reruns the home write gate. A local check result is never an admission result at home |
| B4: cursors | Every cursor, history position and `--at` value carries its store identity. A mini-Quipu cursor refuses at home, and the other way round. The `receive` source maps the mini-Quipu head `local_tx` to the home transaction IDs |
| B5: co-location | Follows from B1. Members that share a batch at home share the mini-Quipu file |
| B6: no write-replay journal | Merge retry is R7 idempotence plus read-back by merge id. No journal is added |
| B7: no semantics weakened in transit | Behaviour that is non-atomic or carries no provenance at home stays that way in the mini-Quipu and after merge. A merge does not make an import atomic, and it does not add provenance to an operation that has none at home |

Phase 3 participant commit is the only thing that would let a fork base or a
merge back span home files. Until it exists and is qualified, those refuse.
This note does not depend on Phase 3 and does not ask for it.

## 4. Identity and provenance of merged writes

What a mini-Quipu records is what can survive a merge. Today a store persists
`transactions.actor`, `transactions.source` and `transaction_auth` (principal,
credential, auth class). The `X-Quipu-Agent`, `-Harness`, `-Model`,
`-Session` and `-Host` headers are classified for completeness at commit
(`src/write_provenance.rs`). Agent and session are taken from a verified
attestation when one is present. The classification itself is not stored per
transaction.

Rules:

1. **Record locally under the normal pipeline.** A mini-Quipu write is admitted
   with the same structured provenance and attestation handling as a home
   write. Its transaction rows hold the writer's actor, source and principal.
   Crew process rules already require the session ID in `source` for session
   captures.
2. **Carry per fact at merge.** As in U2, the merge reads each incoming fact
   with its mini-Quipu transaction (`local_tx`, actor, source, principal).
   That provenance goes into the decision rows and the `receive` transaction
   source, next to the merge id and the incoming identity of oracle §3.3.
3. **Never impersonate.** The home transactions belong to the applier (and the
   reviewer for `decide`). Home `transaction_auth` records the principal that
   authenticated at home. The mini-Quipu writer's principal is carried as data
   about the incoming fact. It never becomes the home transaction's principal
   and never satisfies home authority.
4. **Nothing is invented.** If a field was not stored in the mini-Quipu, it is
   absent after merge. It is not reconstructed from headers on the merge
   request. If the structured header fields must survive verbatim, the store
   must persist them per transaction. That is a generic store change for
   every store (slice M4), not a mini-Quipu special case.
5. **Signed operations.** A mini-Quipu has no replay ledger for a home
   signature domain. A signed home operation cannot be performed in a
   mini-Quipu and then replayed at merge (X10). Per-graph design: one
   authoritative replay owner per signature domain.

## 5. Conflict policy

There is no new conflict policy. A merge back is judged entirely by the karb
oracle:

- Rules R1-R8 (one operator, convergent flagged with `continue`, sequenced
  `receive`/`decide`, cardinality from the shape, no rewriting of ours,
  interval sanity, idempotence, applied/held counts).
- Cases C1-C8, with C7 (ROOT re-claims a value after the fork) and C8
  (idempotence and staleness) being the most frequent for long-lived
  mini-Quipus.
- Use case U2 inputs and the partial modes and merge units of §6.
- Decisions are made through the existing reconcile flow
  (`fork_merge_decisions`, [merge reconcile](merge-reconcile.md)).

**Worked example (Seeds, illustration only).** Two agents each fork the Seeds
board set at cut `k` into their own mini-Quipu. Each claims the same item, so
the `card: one` assignee slot changes in each file. Agent A merges first: a
one-side change (C1), applied at its authored time. Agent B merges second: the
slot is now divergent against the new home value, so it is a C4 conflict that
needs a decision. `atomic` mode holds the whole merge, and `partial-slot` holds
only that slot. Neither agent's local claim was a home claim before its merge,
so no home scheduling decision ever rested on mini-Quipu state. A generic
fixture with any `card: one` slot shows the same behaviour.

## 6. Refusals

A mini-Quipu may not:

| ID | Refused | Why |
|---|---|---|
| X1 | Be the writer of a graph IRI that the home store also writes (a catalog route, `[quipu.graph_sets]` entry, or a writable legacy graph treated as owned) | One writer per graph. Moving a graph is a fenced catalog generation switch in the per-graph design, and then it is a routed owner, not a mini-Quipu |
| X2 | Take part of a catalog graph set as writable members | B1 atomicity |
| X3 | Declare membership by prefix, pattern or template | B2. Exact IRIs only, as in catalog file v1 |
| X4 | Fork from, or merge back into, writable members that span home files | No multi-file fork base or merge until Phase 3 |
| X5 | Fork from another mini-Quipu, or merge into one | The base is always a home cut. Nesting adds a base chain that this design does not specify |
| X6 | Write to a read-only reference member | References are pinned copies. Changes to them are made at home |
| X7 | Be composed into home reads, attached writable, or swapped in by replacing the home file | Merge back is the only way back. Attachments stay read-only |
| X8 | Use its cursors or `--at` values at home, or home cursors locally | B4. Store identity is part of every cursor |
| X9 | Bypass home admission: authority, the write gate under current shapes, base digest or epoch checks | The mini-Quipu's own validation and authority are not trusted at home |
| X10 | Run signed home operations or consume home nonces | One replay owner per signature domain |
| X11 | Merge derived facts | Inference output is not a proposal |
| X12 | Carry application-specific behaviour | Quipu stays generic. An application uses a mini-Quipu only through declared graph sets |

Each refusal writes nothing, names its reason, and leaves the mini-Quipu
and its registry entry as they were.

## 7. Proposed child slices

Each slice can land on its own. Each one ships with positive, negative and
sabotage controls on disposable files, and none enables anything on a served
Quipu.

| Slice | Deliverable | Exit proof |
|---|---|---|
| M1: fork registry entry for a file-backed fork | Home records a mini-Quipu fork: identity, base cut, digest, graph set and pinned revisions. Graph-set resolution against one owner, with refusals X1-X5 | Positive: a whole-set fork from one owner records the expected cut and digest. Negative: partial set, prefix member, multi-file set and routed-writer claim each refuse with nothing written |
| M2: materialize and write locally | Native API and CLI create the file with a fresh `store_id` and `log_epoch`, copy members as of the cut with valid time, and pin shapes. Writes to references refuse (X6). Foreign cursors refuse (X8) | A copied file is not accepted as a mini-Quipu. Local writes pass the pinned gate. Reference writes and home cursors refuse |
| M3: merge back through `merge_core` | Fork promote reads `theirs` from the mini-Quipu file. Base rebuilt and digest-checked at home. One owner savepoint. R7 retry. Base advances after a merge | Oracle C1-C8 and partial-mode fixtures give byte-identical plans for a same-store fork and an equivalent mini-Quipu. A sabotage control that routes through `share_merge` or a separate writer fails. Foreign epoch, base-unavailable and base-mismatch refuse |
| M4: provenance carriage | Per-fact mini-Quipu provenance in decision rows and the `receive` source. Decide, for every store, whether the structured provenance fields are persisted per transaction | The merged home transaction shows the applier as principal and the mini-Quipu writer as carried data. A merge request with forged headers changes no carried field |
| M5: retire | Fence the writer, close the registry entry, verify receipts. Retired forks refuse further merges | A merge after retirement refuses. Home history reads the same with the file deleted |
| M6: worked example | A generic fixture shaped like the Seeds board set: two forks, both merged, a `card: one` conflict decided, plus B1, B3, B4 and B7 rows | The example passes with no application-specific code. The same fixture with renamed graphs passes too |

Out of scope: a mini-Quipu on another host pushing over HTTP. That is the
signed-changeset client path of [local sync mode](local-sync-mode.md) §7 and
§8. Its admission must also land on `merge_core`, and that is that design's
own work.
