# Design: Replica composition — shape mappings, local reasoning and the agent surface

> **Status (2026-10-25): design for review. Nothing here is implemented.**
> This is the companion to `local-sync-mode.md`, which fixes how a scoped
> replica bootstraps, tails, proposes and merges back. That document does not
> say how a replica *composes* what it follows. This one does, in four
> parts: declarative shape mappings between vocabularies (§3), a local
> reasoner whose output stays in companion graphs (§4), every modification
> recorded as its own fact (§5), and the surface an agent uses against its
> replica (§6). Source claims cite `origin/main` 07f1a770. A claim marked
> INFERRED was read from the code but not exercised.

## 1. Goal

A replica follows several graphs, written in vocabularies it does not own,
and should be able to say, in one place, "their `ex:Ticket` is my `my:Task`;
their `ex:assignee` is my `my:owner`". After that:

1. a query in **my** vocabulary sees **their** data;
2. an edit in my vocabulary lands in their graph, valid against **their**
   shapes, through the normal three-way merge;
3. when either side's shape changes, the mapping is flagged for a new
   decision. It never silently follows the changed shape;
4. the reasoning that makes this cheap runs on the replica, and its output
   never travels back as asserted facts;
5. every one of these operations (declare, decide, retire, derive, promote,
   push, merge) is a fact, so "what changed, who did it, on what authority"
   is one query;
6. an agent asks its replica one question and gets an answer in its own
   vocabulary, says how each part is known, and is told exactly what was
   not searched.

Non-goals: pattern subscriptions (still deferred, `local-sync-mode.md` §4);
transitive chains of mappings (§3.6); mapping-driven writes into a graph the
writer has no write scope for; erasure of data already on an offline device.

## 2. What already ships

| Piece | Ships today | Gap for this design |
|---|---|---|
| Instance alignment | `align_propose` / `align_decide` / `align_apply` (`src/mcp/align.rs:47,100,139`). Apply writes `owl:sameAs` and `quipu:distinctFrom` knots, each with `align:assertedBy`, `align:assertedOn` and `align:justification`, in one `transact_to_graph` into `urn:quipu:align:<12hex>:<12hex>` (`src/align/apply.rs:87-96,135-172,345-351`). Apply needs `expected_version` = sha256 of the SSSOM set (`apply.rs:78-80,303-309`). | Aligns **instances**, not classes or properties. Nothing pins a set to a shape version, and nothing reacts when a shape changes. INFERRED: the SSSOM rows themselves are not stored as RDF, only the knots. |
| Equivalence-aware reads | `docs/design/equivalence-aware-reads.md` (status: proposed). Its rehearsal measured four cross-namespace class reads returning **0** through a SHACL-registry equivalence bridge; only explicit `UNION`+`DISTINCT` returned the row. It proposes an opt-in, versioned `alias_read` over graph and ontology digests. | No implementation: `src/sparql` has no equivalence handling. Equivalence only, with no one-way inclusion. |
| RDFS class expansion | `collect_class_and_subclasses` (`src/sparql/rdfs.rs:26`) | Reads subclass axioms from the store, not from a scoped mapping. |
| OWL materialization | Equivalence materialized into the companion graph (`src/owl.rs:41-100`), with explanations (`src/explain.rs:393,451`) | Applies store-wide. It is not tied to a mapping or a mapping version. |
| Shape versions | `shapes` / `ontologies` are bitemporal registry **tables**, keyed `(name, valid_from)` with `valid_to` and `tx` (`src/store/registry.rs:19-114`). As-of reads exist. A load writes a `shapes.loaded` event row carrying its tx (`registry.rs:195-221`). The as-of share manifest carries a combined `shapes_hash` (`src/asof_share.rs:358`). | No digest for an individual version (INFERRED: a version is named only by `(name, valid_from)`). Shape changes are event rows, not facts, and they do not travel on the replication stream. |
| Companion graphs | Reserved suffix `#inferred` (`src/store/inferred.rs:36`); a write guard refuses non-engine writers (`inferred.rs:83-113`); graph-level `aegis:sourceKind "inferred"` and `derivedAsOfTx` (`inferred.rs:43,47,195`). Companions are interned, not registered (`inferred.rs:125-135`), so the as-of share refuses them as unregistered graphs (`src/asof_share.rs:334-336`). | That refusal is a side effect, not a stated rule. `derivedAsOfTx` is a local tx, not a source-qualified cursor. Promotion out of a companion is pending (`entailment-regime.md` §3). |
| Reasoners | Datalog engine with retraction propagation (`docs/design/reasoning-engine-fixes.md`); `ReactiveReasoner` behind the `reactive-reasoner` feature; the wasm explorer compiles reactive Datalog (`docs/design/wasm-support.md`) | OWL support-loss handling is pending, so OWL-derived facts may not retract correctly (`entailment-regime.md`). |
| Operation records | Key registration and revocation are bitemporal facts (`src/governance/verifier_registry.rs`, `trust_root.rs`); episodes carry `prov:wasGeneratedBy` (`src/episode/mod.rs:563-573`); alignment decisions are facts (above) | A merge is only a tx `source` string, `share-merge:parents=a,b` (`src/share_merge.rs:303,342`). Conflicts are response structs (`DecisionRecord`, `share_merge.rs:31`). There is no changeset receipt, promotion record or subscription record. |
| Agent surface | `quipu_graph_list` (`src/mcp/definitions/core.rs:303`). Search carries the `quipu-context/v1` contract: `insufficient_context` plus `outcome: "NO CONTEXT"` and a reason (`src/mcp/tools/search_confidence.rs:79-93`). | `quipu_query` and `quipu_context` have no absence contract. Nothing reports which graphs were not searched, or how fresh the answer is. |

## 3. Shape mappings

### 3.1 A mapping is facts in a graph the replica owns

A **mapping** is a set of schema-level correspondences between a *source*
vocabulary (theirs) and a *target* vocabulary (mine). It is stored as
ordinary facts in a registered, committed **mapping graph**, which the
replica owns. Because it is an ordinary graph, it replicates, merges back
and shows up in history like any other. A mapping graph may hold one
mapping or several.

Each mapping is a node of class `quipu:ShapeMapping` with:

- `quipu:mapsFrom` and `quipu:mapsTo`: the source and target graph sets
  the mapping applies to. A mapping never applies outside them, so it
  cannot leak into an unrelated graph that happens to share a class IRI.
- `quipu:pinsShape`, one or more. Each pin names one shape version on
  either side: `(shape name, valid_from, version digest)` (§3.4).
- One `quipu:correspondence` per row (below).
- `quipu:mappingStatus`: `proposed`, `decided`, `flagged` or `retired`.
- `quipu:decidedBy` / `quipu:decidedAtTx`: the decider's principal and the
  transaction that recorded the decision.
- `quipu:mappingDigest`: sha256 over the canonical list of rows and pins,
  in the same way `set_version` is computed for alignment sets
  (`src/align/apply.rs:78-80`). Every read or write that uses a mapping
  reports this digest.

**Correspondence rows** reuse the SSSOM columns that the alignment model
already parses (`src/align/sssom.rs:52-67`): `subject_id`, `predicate_id`,
`object_id`, `mapping_justification`, `author_id`, plus `quipu_review`. The
subject is a source term and the object a target term. Only these
`predicate_id` values are allowed in the MVP:

| `predicate_id` | Kind | Read | Write-back |
|---|---|---|---|
| `owl:equivalentClass` | class ≡ class | both directions | yes |
| `rdfs:subClassOf` | their class ⊑ my class | their instances answer my class | **no**: my class does not say which of their classes to write |
| `owl:equivalentProperty` | property ≡ property | both directions | yes |
| `rdfs:subPropertyOf` | their property ⊑ my property | their values answer my property | **no** |
| `owl:inverseOf` | property ≡ inverse of property | both directions | yes, with subject and object swapped |

Anything else is refused at declaration. No literal transforms, no
value maps and no SPARQL-defined correspondences are allowed: those cannot
be inverted soundly, and §3.5 depends on inversion.

### 3.2 Lifecycle reuses the alignment discipline

Mappings go through the same three steps as instance alignment: **propose**
(a read; it may suggest rows from class and property labels), **decide** (a
read that returns the rows with review marks and the digest) and **apply**
(the only writer, which needs `expected_digest`). These are new tools,
`quipu_mapping_propose|decide|apply`, beside `quipu_align_*`. They are not
new modes of those tools: alignment knots are `owl:sameAs` over instances,
and mixing schema rows into that set would change what `set_version`
means for existing users.

Apply writes one transaction. It contains the mapping node, its rows, its
pins and the status `decided`, plus the operation record (§5). Re-applying
the same digest is a no-op that returns the same record.

**Retire** sets `valid_to` on the mapping node and its rows (valid time),
in one transaction with its record. It does not retract them. "Which
mapping was in force when this edit was made?" is then an as-of read.

### 3.3 Read: rewrite or materialize, chosen per mapping

Each mapping declares `quipu:applyMode`:

- **`rewrite`** (default, the least memory). A query over the target
  vocabulary is answered from the mapped source graphs through a read-only
  virtual view. Equivalence rows feed the `alias_read` relation construction
  proposed in `equivalence-aware-reads.md` §4. Inclusion rows (`⊑`) feed the
  same class and property expansion `collect_class_and_subclasses` does
  today (`src/sparql/rdfs.rs:26`), but with the mapping graph as the only
  axiom source and the mapping's `mapsFrom` graphs as the only data. The
  answer reports the mapping digest. **This needs `alias_read` to be built.**
  It is the first dependency of this design (§8, C2).
- **`materialize`** (the fastest reads). The replica's reasoner derives the
  target-vocabulary facts into the mapping graph's companion
  (`<mapping graph>#inferred`) (§4). Reads then name the companion like any
  other.

Both modes must give the same answer on the same fixture (§7, A1). A mode
that silently returns nothing counts as a failure, as
`equivalence-aware-reads.md` already requires of `alias_read`.

### 3.4 Pinned to shape versions; a change flags, never extends

**New: a digest for each shape version.** A pin needs to name content, not
only a time. The `shapes` and `ontologies` registry rows gain a `digest`
column: sha256 over the canonical form of the shape text, computed at load.
A version is then `(name, valid_from, digest)`. The combined `shapes_hash`
in the as-of share manifest stays as it is, and is derived from the same
canonical forms.

**New: shape changes on the stream.** Shape loads are registry rows, not
graph facts, so the replication stream does not carry them today. The
stream gains a `control` frame `shapes {name, valid_from, valid_to,
digest, tx}`, emitted at the transaction that changed the shape set. It is
sent on every session whose manifest declared that shape (in its scope or
its context graphs). Like revocation, it is not graph-filtered
(`local-sync-mode.md` §4). A replica that reconnects after a shape change
learns of it from the manifest's `shapes_hash` on resume.

**Flagging.** When a version pinned by a `decided` mapping is closed (on
the master, at `shapes.loaded` or `shapes.removed`; on a replica, at the
`shapes` frame), the mapping is set to `flagged` in a transaction that also
writes the operation record (§5), and a `mapping.flagged` event names the
mapping, the old pin and the new version. A flagged mapping:

- stops being applied: rewrite reads leave it out, and materialized output
  is dropped (§4.4);
- reports `flagged` in every answer that would have used it (§6);
- is never extended or re-pinned automatically. It comes back only through
  a new **decide/apply** that pins the new version and gets a new digest.

The master is authoritative for flagging. A replica that flags locally (it
saw the `shapes` frame first) writes no `flagged` fact of its own. It
applies the flag locally and waits for the master's record to arrive on the
tail.

### 3.5 Write-back translation

An edit made in my vocabulary, to a subject that lives in a mapped source
graph, is translated **on the replica, before the changeset is built**:

- Each asserted or retracted quad whose predicate (or `rdf:type` object) is
  a target term is rewritten through the mapping's invertible rows
  (§3.1, last column) into the source vocabulary and the source graph.
- A quad that would need a non-invertible row (`⊑`) is **refused locally**
  with "not translatable through mapping M: my:Task has 2 source classes".
  It is never guessed.
- A quad on a target term no mapping covers stays in my own graph,
  unchanged.

The changeset therefore carries **their** vocabulary, in **their** graph.
The master never needs to understand the mapping to admit it, and validates
it against **their current shapes** in the accepting transaction, as
`local-sync-mode.md` §7 already requires. The envelope gains one optional
field, `translated_by: {mapping IRI, mapping digest}`, which the receipt
echoes (§5). If any pin in that mapping no longer matches the master's
current shape version, the master returns a **conflict**
(`DecisionRecord` reason `mapping-stale`) rather than a refusal, as for a
shape-version mismatch in §13 Q3 of `local-sync-mode.md`. The overlay
survives, and the client re-decides or rebases.

Writing into the source graph needs write scope on that graph. A mapping
grants no authority.

### 3.6 Deliberately excluded

- **Chains.** A mapping's source terms must not themselves be the target of
  another decided mapping in the same scope. This keeps translation one
  hop, so it can be inverted, and keeps the relation finite. Declaring a
  chain is refused, and the refusal names the existing mapping.
- **Conflicting mappings.** Two decided mappings in one scope whose rows send
  the same source term to different target terms are refused at apply.

## 4. Local reasoning, isolated in companion graphs

### 4.1 Where the output goes

A replica runs the reasoner (Datalog, and `ReactiveReasoner` where the
feature is built) **locally**, over the graphs it follows. This is part of
the offload goal: the master does not compute derivations for the
replica's working set.

- Ordinary derivations over a followed graph `G` go into `G#inferred`,
  through the existing companion path and write guard
  (`src/store/inferred.rs:83-135`).
- Derivations produced by a materialized **mapping** go into
  `<mapping graph>#inferred`, never into the source graph's companion. When
  a mapping is re-decided or flagged, its companion is dropped and
  re-derived as a whole (§4.4). It is never patched, and dropping it touches
  nothing else.

### 4.2 Companions never replicate, in either direction

This becomes a stated rule rather than a side effect of the as-of share
refusing unregistered graphs (`src/asof_share.rs:334-336`):

- An as-of share, a stream session and a relay **refuse** any scope that
  names a `#inferred` graph, with a stated reason: "companion graphs are
  derived locally; follow the base graph".
- The changeset builder **refuses** any quad in a `#inferred` graph, and the
  master's admission refuses it again. This puts into code the rule that
  "local inference writes into a separate derived graph that is never
  included in a changeset" (`local-sync-mode.md` §7).
- Each replica therefore derives its own companions from its own canonical
  state, and the master's companions are the master's.

### 4.3 Freshness that compares with the cursor

`derivedAsOfTx` is a local transaction id today (`inferred.rs:43,195`). On a
replica it must be comparable with the applier's cursor, which is
source-qualified (`local-sync-mode.md` §5). The companion therefore gains
`quipu:derivedAsOfSource`: `{store_id, log_epoch, source_tx}`, one per
source the premises came from, written in the same transaction as the
derivation.

A companion is **stale** when any of its premise sources has a cursor beyond
`derivedAsOfSource`, or when its `log_epoch` differs from the cursor's. The
stale flag is reported with each answer that read the companion (§6). It is
never dropped silently.

### 4.4 Retraction and re-decision

- An upstream retraction arrives as an ordinary applied transaction. The
  reactive reasoner sees that delta, and retraction propagation removes the
  dependent derivations (`reasoning-engine-fixes.md`).
- **Limit:** OWL support-loss handling is still pending
  (`entailment-regime.md`). Until it lands, `materialize` mappings run as
  **Datalog rules only**, generated from the mapping rows (one rule per
  row, e.g. `my:Task(x) :- ex:Ticket(x)`). They do not go through
  `ReactiveOwl`, so every derived fact can be retracted. OWL reasoning over
  followed graphs is still allowed, but its companion is labelled
  `retraction: unverified` until that work lands.
- An epoch change on any premise source drops and re-derives the companion,
  as the applier rebuilds canonical state (`local-sync-mode.md` §5).

### 4.5 Promotion is a changeset with a record

Anything worth keeping from a companion comes back only through promotion
(`entailment-regime.md` §3). On a replica, a promotion is a changeset
whose quads are the promoted facts, now *asserted*, in a base graph, plus a
promotion record (§5) naming the rule, the derivation and the authority.
The master admits it only if the authority check in `entailment-regime.md`
§3 passes. **Until that authority API exists, the master refuses promotion
changesets** (`DecisionRecord` reason `promotion-unsupported`), and the
derivation stays in the companion.

## 5. Every modification is its own fact

Each operation that changes what is true or trusted writes a record node
**in the same transaction** as the change itself, so a change without a
record cannot exist. Records live in a registered **operations graph** on
the master (configurable, default `urn:quipu:graph:operations`), which is
governed by the same read authorization as any graph and is outside every
replica scope by default. A replica's own records travel to the master
inside its changesets, and are then re-recorded by the master's receipt.

| Operation | Record class | Fields (beyond actor, `tx`, time) | Today |
|---|---|---|---|
| changeset admitted, refused or conflicted | `quipu:ChangesetReceipt` | operation id, signer, base `{store_id, log_epoch, base_tx, base_slice_hash}`, delta hash, signature, outcome, accepted tx, `translated_by` | designed in `local-sync-mode.md` §8; not built |
| merge performed | `quipu:MergeRecord` | base, both parents, conflict list, resolver and resolver signature, validation evidence | only the tx `source` string (`src/share_merge.rs:303,342`) |
| promotion out of a companion | `quipu:PromotionRecord` | the promoted quads' hash, rule IRI, derivation (premise facts and source cursors), authority | absent; blocked on `entailment-regime.md` §3 |
| mapping declared, decided, flagged or retired | `quipu:MappingRecord` | mapping IRI, old and new digest, pins on both sides, old and new status, the shape version that caused a flag | alignment decisions only (`src/align/apply.rs:152-172`) |
| key registered or revoked | `aegis:VerifierRegistration` / `aegis:RegistryAmendment` | already defined | **ships** (`src/governance/verifier_registry.rs`, `trust_root.rs`) |
| subscription opened or closed | `quipu:SubscriptionRecord` | principal, scope digest, policy digest, `log_epoch`, open/close cursor, close reason | stream sessions are registry rows (INFERRED, `src/store/push.rs:47,112`); **opt-in** per scope, as access audit is not always worth a write per session |

Rules:

- A record is written by the code path that makes the change, not by a
  watcher afterwards. The write guard refuses a record class written from
  any other source, in the same way the companion guard refuses non-engine
  writers.
- Every record carries `quipu:onAuthority`: the registration, grant or
  decision that allowed it. Something that cannot name its authority is
  refused, not recorded as "ungoverned".
- One stored query, `what_changed(subject, since)`, answers across all
  record kinds: what changed, who did it, on what authority, and in which
  transaction. "Trusted when merged" and "trusted today" are the same query
  at two valid times, as for keys today.
- Each new record class ships with its SHACL shape in the same change.

## 6. The agent surface on a replica

The agent is the main client. It must not need to know the graph layout to
get a complete answer.

1. **One entry point.** `quipu_search`, `quipu_query` and `quipu_context` on a
   replica default to the **replica scope**: every followed graph, their
   companions, and the target view of every decided mapping. An explicit
   `graph` argument narrows the scope and is never needed for completeness.
2. **How we know.** Every row or hit carries a compact provenance line:
   `asserted | derived | mapped`, the source graph, the source-qualified
   cursor it was read at, and flags (`stale`, `flagged`,
   `retraction: unverified`). A mapped row names the mapping digest. The
   line is designed for a token budget: one short line per row, and fields
   identical across a whole answer are hoisted to a header once.
3. **Honest absence.** The `quipu-context/v1` contract
   (`src/mcp/tools/search_confidence.rs:79-93`) extends from search to query
   and context on a replica. An empty answer returns
   `outcome: "NO CONTEXT"` with `searched` (the graphs and mappings
   actually read), `not_searched` (graphs the principal may list on the
   master but does not follow, as a count when it may not see their names),
   and `unknown` (context graphs the manifest needed but does not hold,
   from `local-sync-mode.md` §4). It never returns a bare empty result.
4. **Search first, SPARQL second.** Natural-language and structured search are
   the default path and carry everything above. SPARQL is the escape hatch
   and carries the same provenance and absence header.
5. **Catalog.** `quipu_graph_list` on a replica adds, for each graph:
   `followed` (yes or no), scope membership, cursor and freshness from the
   applier, the shape-set versions in force, and the mappings that read from
   or write to it, with their status. A role primer in `quipu_context`
   names the agent's followed scope and its mappings in a few lines.
6. **Write status in plain words.** A write returns `merged` (with the
   receipt), `pending merge` (queued, with operation id) or `refused`. A
   refusal or conflict renders the `DecisionRecord` as a sentence naming
   the shape, the constraint and the value, e.g. "ex:Ticket requires
   ex:assignee; my:owner maps to it, but no value was given".
   Translation refusals (§3.5) read the same way.
7. **Local and fast.** Hot reads never go to the master. A read that cannot be
   answered locally returns UNKNOWN with the reason. It does not fall back to
   the master in silence.

## 7. Acceptance (falsifiable)

All fixtures are private and in-process (native, and wasm where noted). Each
control must fail with its mechanism disabled.

- **A1 — two-vocabulary read.** Graph B uses `ex:Ticket` / `ex:assignee`;
  replica R works in `my:Task` / `my:owner` with a decided mapping. A query
  for `my:Task` with `my:owner` returns the B entities with their mapped
  owner, in both `rewrite` and `materialize` modes, with **identical
  bindings**. With the mapping removed, the same query returns
  `NO CONTEXT` naming B as searched and the mapping as absent. It never
  returns a bare zero. This is the case `equivalence-aware-reads.md`
  measured at 0.
- **A2 — write-back.** On R, asserting `my:owner` on a B entity produces a
  changeset whose quad is `ex:assignee` in graph B. The master admits it
  against B's current shapes, and a fresh as-of read of B shows it. Asserting
  `my:Task` on a new subject when two source classes are mapped `⊑` is
  refused locally as not translatable. A changeset carrying a stale mapping
  digest is a `mapping-stale` conflict with the overlay intact.
- **A3 — shape change flags.** Loading a new version of B's `ex:Ticket`
  shape on the master flags the mapping (a `MappingRecord`, a
  `mapping.flagged` event, a `shapes` frame to R). A1's query then reports
  `flagged` and stops using the mapping. Nothing re-pins without a new
  decide/apply.
- **A4 — companion isolation.** With `materialize`, the derived `my:Task`
  facts exist only in `<mapping graph>#inferred`. A changeset built after
  local work carries none of them, and a hand-built one that includes them
  is refused by the builder and by the master. A scope naming a `#inferred`
  graph is refused by share, stream and relay.
- **A5 — retraction.** Retracting the B entity's `rdf:type ex:Ticket` on the
  master removes the derived `my:Task` from R's companion after R applies the
  tx. Before R applies it, the answer is flagged `stale` against the cursor.
- **A6 — one audit query.** After A1–A5, `what_changed` over B and the mapping
  lists the mapping decide, the flag, the admitted changeset (with
  `translated_by`) and the merge, each with actor, authority and tx.
  Deleting a record row by hand, outside the writing path, is refused.
- **A7 — competency eval.** A set of agent tasks whose answers need facts from
  a followed graph, a companion and a mapped vocabulary, with no graph hints
  in the task text. Success rate and tool calls per answer are measured from
  session telemetry, against the master today and then against a replica.
  The design is accepted when the replica is no worse on success and needs
  fewer calls. The numbers are recorded, not assumed.

## 8. Phasing

Each phase is its own source change, merge, activation and rollback, and
follows the corresponding `local-sync-mode.md` phase where it says so.

| Phase | Deliverable | Depends on | Acceptance |
|---|---|---|---|
| **C1** mappings on the master | per-version shape digests; `ShapeMapping` facts; `quipu_mapping_*`; flagging and `MappingRecord` | none; master-only, no replica needed | A3 on the master; refusal controls for chains, conflicts and disallowed predicates |
| **C2** rewrite reads | `alias_read` (from `equivalence-aware-reads.md`) plus scoped inclusion expansion | C1 | A1 in `rewrite` mode on the master |
| **C3** replica reasoning | local reasoner over followed graphs; mapping rules into mapping companions; `derivedAsOfSource`; the companion refusal on share, stream and relay; the `shapes` control frame | C1, `local-sync-mode.md` P2 | A1 `materialize`, A4, A5 |
| **C4** write-back | translation in the changeset builder; `translated_by`; the `mapping-stale` conflict | C1, `local-sync-mode.md` P3 | A2 |
| **C5** operation records | `ChangesetReceipt` (with P3), `MergeRecord`, opt-in `SubscriptionRecord`, `what_changed`; `PromotionRecord` only once the promotion authority exists | `local-sync-mode.md` P3; `entailment-regime.md` §3 for promotion | A6 |
| **C6** agent surface | replica default scope, provenance line, absence contract on query and context, replica catalog, plain-word write status | `local-sync-mode.md` P2 and P4 | A7 before and after |

Merging this document approves the direction only. It grants no
benchmark, production replay or activation.

**Rollback.** Each phase keeps the `local-sync-mode.md` §11 rule:
accepted master history is never rewound.

- C1 rolls back by disabling the mapping tools. The mapping facts and
  records already written stay as history.
- C2 and C3 roll back by leaving every mapping unapplied. Answers fall back
  to plain graph reads with `NO CONTEXT` where the mapping was needed, and
  companions are disposable.
- C4 rolls back by refusing translation in the builder. Overlays are kept.
- C5 and C6 are additive. Turning them off loses no data.

## 9. Open questions

1. **Who may decide a mapping that is used for write-back?** Recommendation:
   anyone may decide a mapping that is only *read*, in a graph they own. A
   mapping named in `translated_by` must be registered on the master and
   decided by a principal with write scope on the source graph. Otherwise
   the master returns a conflict.
2. **Mapping graph placement.** Recommendation: one mapping graph per
   replica owner, followed back by the master like any proposal target, so
   mappings are shared and reviewable. The alternative is mappings kept
   local to the replica (no merge-back), which is simpler but leaves them
   invisible to the audit query.
3. **Subscription records on by default?** Recommendation: off by default,
   on for any scope whose policy is labelled access-audited. The cost is one
   write per session open and close on the master.
4. **Chains later?** Excluded here (§3.6). Revisit only with a measured need,
   and only for equivalence rows, whose composition stays invertible.
