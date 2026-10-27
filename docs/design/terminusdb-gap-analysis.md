# TerminusDB and WOQL vs. Quipu: comparison and gap analysis

> **Status: analysis, 2026-10-27.** This compares [TerminusDB][tdb-repo] v12.0.8
> (commit [`0b5549a`][tdb-commit], Apache-2.0; the source tree was read
> directly, along with the 12.1.0-rc1 release notes) with Quipu `0.16.4` on
> `main` at `ef94a93`.
>
> - **Quipu claims** were checked against source and tests, not README or
>   design-doc ambition. Where a design doc and the code disagree, the code
>   wins and the disagreement is noted.
> - **TerminusDB claims** come from their source tree (links pin `0b5549a`),
>   their docs ([terminusdb.com/docs][tdb-docs] and the
>   [`llms-full.txt`][tdb-llms] corpus), and GitHub metadata.
>
> The gaps below are proposals, not commitments. Each should become a tracked
> item before anyone implements it. This document stays consistent with the
> merge paper's related-work section (`docs/paper-merge/sections/08-related.tex`):
> TerminusDB solves the *representation and transport* of a delta, and Quipu's
> contribution is deciding *which deltas are irreconcilable*.

## One-line framing

**TerminusDB** is a versioned document graph. JSON documents with a
closed-world class schema are stored as RDF triples in immutable,
succinct-encoded delta layers, one per commit. On top sit git-style
operations: branch, diff, patch, apply, rebase, squash, and clone, fetch, push
and pull between instances. The query layer is a Prolog-compiled Datalog (WOQL)
plus a GraphQL API auto-generated from the schema.

**Quipu** is a governed bitemporal RDF store. An EAVT fact log carries valid
time and transaction time, behind a SHACL/OWL/authority write gate that
refuses bad writes. Queries are SPARQL 1.1. Merge is shape-aware three-way,
and the shapes graph decides what is a conflict. Agents reach it over MCP and
REST.

Both are "RDF underneath, governed by a schema, with history". They differ in
four ways:

- **Time:** TerminusDB has commit time only. Quipu has valid time plus
  transaction time.
- **Constraints:** TerminusDB has a class-and-cardinality schema. Quipu has
  SHACL Core.
- **Query standard:** TerminusDB has none (WOQL is proprietary). Quipu has
  SPARQL.
- **Version control:** TerminusDB's git verbs are complete and ergonomic.
  Quipu's branch story is split across forks, overlays, shares and git, and
  one of those paths has a real bug (G1).

## Comparison at a glance

| Capability | TerminusDB 12.0.8 | Quipu 0.16.4 | Evidence |
|---|---|---|---|
| Language, license | Prolog (SWI) + Rust storage, Apache-2.0 | Rust, SQLite, Apache-2.0 | [tdb `src/rust/Cargo.toml`][tdb-cargo]; quipu `Cargo.toml` |
| Data model | JSON documents ↔ triples in separate instance, schema and inference graphs. IRIs, xsd/xdd literals | RDF quads over an EAVT log `facts(e,a,v,g,tx,valid_from,valid_to,op)` | [tdb `json_rdf.pl`][tdb-jsonrdf]; quipu `src/schema.rs:50-81` |
| RDF import/export | Graph-level Turtle via `/api/triples`. Document-level JSON-LD, Turtle and RDF/XML are **Enterprise only** (the community build rejects them) | Native: N-Triples, Turtle and other `oxrdfio` formats, plus qpack and share artifacts | [tdb `routes.pl:376-438`][tdb-routes], [`api_document.pl:56-108`][tdb-apidoc]; quipu `src/rdf.rs`, `src/pack.rs` |
| Storage | `terminus-store`: immutable succinct delta layers, rollup and optimize, about 13 B/triple claimed. Queries run in memory | SQLite (WAL) with AEVT/VAET/GEAV indexes. Optional in-memory read model capped at 1M triples | [delta rollup docs][tdb-rollup]; quipu `src/schema.rs`, `docs/design/in-memory-read-model.md` |
| Time | **Commit time only.** Read any commit by descriptor. `AddedTriple` and `DeletedTriple` query one layer's delta. No valid time | **Bitemporal.** `valid_at` + `as_of_tx` on every read, `retracted_tx`, contradiction detection over overlapping valid intervals | [tdb `ref.json`][tdb-ref]; quipu `src/store/reads.rs:543-590`, `src/sparql/mod.rs:223` |
| Retraction | Delete documents or triples in a new commit | Close `valid_to` or write `op=0` (nothing is deleted). Episode retraction with an orphan policy | quipu `src/store/retract_triples.rs`, `src/store/ops.rs:509-560` |
| Schema language | Classes, `@inherits`, `@abstract`, `@subdocument`, Enum, TaggedUnion, Optional/Set/List/Array, Set cardinality, key strategies (Lexical, Hash, ValueHash, Random, UuidV7) | SHACL Core (rudof) + OWL 2 RL subset + RDFS. No key strategies | [tdb `json.pl:1699-1772`, `:2668-2684`][tdb-json]; quipu `src/shacl.rs`, `src/owl.rs` |
| Validation | Closed world, at commit, all-or-nothing | Refusal at write, with per-shape `onViolation` reject/emit and structured feedback. 98/98 W3C SHACL Core cases. `sh:sparql` is a non-goal | quipu `src/shacl_admission.rs`, `docs/book/src/benchmarks/conformance.md` |
| Value constraints | **None**: no pattern, min/maxInclusive, length or property-pair constraints. Restrictions in GraphQL filter only | Full SHACL Core value constraints | [schema reference][tdb-schema]; [tdb `restrictions.pl`][tdb-restr] |
| Schema migration | Typed operations: CreateClass, MoveClass, CastClassProperty, ChangeKey and others, plus automatic migration | Bitemporal shape and ontology registry, schema-change **proposals** (shape/ontology/class/property) with accept/reject. No data-migration operations | [migration reference][tdb-migr]; quipu `docs/design/shape-versioning.md`, `src/proposal.rs` |
| Branch | First-class branches on every database | Forks: a committed named graph built from ROOT as of a transaction (create, list, diff, drop, promote; CLI only). Overlays: copy-on-write views | [tdb `routes.pl`][tdb-routes]; quipu `src/store/forks.rs`, `src/store/overlays.rs` |
| Commit graph | **Linear**: `Commit.parent` is `Optional`, so there are no merge commits | None internal. The transaction log is linear and git supplies the DAG for qpacks | [tdb `ref.json:70`][tdb-ref]; quipu `src/git_merge.rs` |
| Diff / patch | JSON document diff, patch ops (SwapValue, ForceValue, PatchList and others), standalone `/api/diff` and `/api/patch`, `match_final_state` | `fork_diff` (set difference), `share_diff`, `share_delta` (SPARQL Update deltas), `/changes` feed with old and new values. No RDF Patch format | [tdb `patch.pl:265-358`][tdb-patch]; quipu `src/share_diff.rs`, `src/share_delta.rs`, `src/store/changes.rs` |
| Merge | `/api/apply`: a two-way diff from a **caller-supplied** common ancestor, applied as one squash commit. Passing the target tip silently reverts target-side changes ([their docs say so][tdb-merge]). Conflicts return 409 with witnesses. The only schema awareness is that the result must validate | **Shape-aware three-way** (`share_merge`): a slot conflicts only past `sh:maxCount`. Decision records, reviewer-applied decisions, and a git merge driver. 33 conflicts vs 845 from the context-overlap baseline (merge paper) | [tdb `api_patch.pl`][tdb-apipatch], [`apply.pl`][tdb-apply]; quipu `src/share_merge.rs:211-305`, `src/git_merge_repo.rs`, `docs/paper-merge/` |
| Rebase / squash / reset | Yes. Rebase revalidates each step, with a `fixup(Message, Woql)` strategy | No | [tdb `db_rebase.pl:92-124`][tdb-rebase] |
| Instance ↔ instance | clone, fetch, push and pull over HTTP. pack/unpack | Pull side only: `quipu follow` read replicas, scoped replication stream, relay chains, as-of share cuts, `/acquire`. **Push-back is design only** | quipu `src/follow.rs`, `src/asof_share.rs`, `docs/design/local-sync-mode.md` |
| Query language | **WOQL**: JSON-LD query documents, regular path queries, And/Or/Opt/Not/GroupBy, recursion through tabled named queries, writes inside queries. JS and Python builders. **No SPARQL** | **SPARQL 1.1**: query 167/168, Update 93/93, Protocol 34/34 (W3C). Property paths, aggregates, configured SERVICE. Datalog rules (`rule_query`) and stored parameterized queries | [tdb `woql.json`][tdb-woql], [`woql_compile.pl`][tdb-woqlc]; quipu `src/sparql/`, `src/reasoner/compile.rs`, `src/store/queries.rs` |
| GraphQL | Auto-generated per database: filters, back-links, path fields, mutations. Subscriptions in 12.1-rc | None | [tdb `graphql/`][tdb-gql] |
| Reasoning | Inference graph (schema-level) | OWL 2 RL subset, RDFS in SPARQL, and a Datalog reasoner (datafrog, stratified negation) that writes to a quarantined companion graph | quipu `src/owl.rs`, `src/reasoner/` |
| Search | 12.0.8 only renders embeddings. 12.1-rc adds in-process "VectorLink" hybrid vector + full-text search with one snapshot per commit; the model is locked per store | FTS5 lexical + trigram, brute-force cosine (<1M facts), LanceDB optional, RRF hybrid, bobbin delegate | [tdb `vectorsearch/`][tdb-vec], [versioned search][tdb-vsearch]; quipu `src/lexical.rs`, `src/vector.rs`, `src/mcp/tools/search_fusion.rs` |
| Access control | Users, organizations, roles and capabilities per organization or database. No per-document or per-branch ACL | Bearer token + read-only mode, per-graph write authority (narrows along the call chain), per-graph read authorization (opt-in) | [tdb `system_schema.json`][tdb-sys]; quipu `src/governance/authority.rs`, `src/read_authorization/` |
| Provenance, signing | Commit author, message and timestamp | Per-transaction actor + source + authenticated principal. ed25519/COSE/SSH/WebAuthn verdicts, signed writes, trust-label lattice | quipu `src/schema.rs:20-33`, `crates/quipu-signing`, `src/lattice.rs` |
| Agent surface | 12.1-rc adds a local MCP server. Agent-memory docs page, `llms.txt` | 81 MCP tools, about 117 REST routes, WASM explorer, stdlib Python client | [agent page][tdb-agents]; quipu `src/mcp/definitions*` |
| Replication / HA | Single node. No replication or HA documented | Read-only followers with `/replica/ready`, no leader election or multi-writer. Read-side federation | quipu `src/follow.rs`, `src/follow_ready.rs`, `src/provider/remote.rs` |
| Published performance | None found, beyond the 13 B/triple claim and "≥4 GB RAM" guidance | Load baseline p99 81.7 ms at 151 MB RSS. Ingest 3,832 facts/s published. #885 cut board GROUP BY from 583→193 ms (10k) and 2,908→826 ms (50k) | quipu `benchmark/load-test-baseline.json`, `docs/book/src/benchmarks/README.md`, commit `ef94a93` |
| Project health (last 6 months) | 292 commits on main, 238 of them from one maintainer (DFRNT/Gettier). Releases 12.0.6 → 12.1.0-rc1. 3.46k stars, 7 open issues. Stewardship moved to DFRNT in 2025; TerminusCMS was retired into "TerminusDB Cloud" (early access) | Active, multi-agent crew, 3,704 `#[test]` functions | `gh api`; [cloud docs][tdb-cloud] |

**A correction to our own record.** `docs/design/vision.md:313` says TerminusDB
has "no vector search, no agent integration". That was true when written. It
is no longer true of 12.1-rc, which ships versioned vector and full-text search
plus an MCP server, and markets itself for agent memory. The vision table
should be updated the next time it is touched.

## Where Quipu is ahead

These are specific, so the gap list reads in proportion.

- **Valid time.** TerminusDB can answer "what did the database say at commit
  X". It cannot answer "what was true on March 15 according to what we knew on
  April 2". Its own docs use "bitemporal" only for dashboard revision history.
  For agent memory this matters: corrections, late-arriving facts and
  supersession need both axes. Commit history as temporal memory, which their
  agent page sells, confuses when we learned something with when it was true.
- **Constraint expressiveness.** SHACL Core (98/98 W3C cases) covers patterns,
  value ranges, lengths, property pairs, qualified shapes and closed shapes.
  TerminusDB's schema stops at type, cardinality and key. A rule like "a
  `Host` IP matches this pattern" or "`end` is after `start`" cannot be
  enforced at commit in TerminusDB.
- **Refusal semantics.** Both reject invalid writes. Quipu also offers
  per-shape emit vs reject, structured repair feedback, authority gates,
  quarantine for derived facts, and a `write.refused` record. TerminusDB
  returns a validation error.
- **Merge semantics.** TerminusDB's apply is two-way diff plus patch, from an
  ancestor the caller must find, and a wrong ancestor silently reverts work
  ([documented][tdb-merge]). Conflicts are positional ("this value changed on
  both sides"). Quipu's `share_merge` derives the conflict predicate from
  `sh:maxCount`: concurrent additions to a multi-valued slot are not a
  conflict, and a second value on a functional slot is. That is the merge
  paper's thesis (33 vs 845), and TerminusDB has nothing like it.
- **Standards.** SPARQL 1.1 at near-full W3C conformance, RDF in every format
  in the open build, SHACL as the schema. TerminusDB has no SPARQL, and puts
  document-level RDF formats behind Enterprise. Quipu data leaves in a
  standard shape; TerminusDB data leaves as JSON or graph-level Turtle.
- **Provenance and signing.** Per-transaction source and principal, episode
  retraction, signed verdicts and writes, trust labels. TerminusDB has commit
  author and message.
- **Read scale-out.** Followers, relay chains, as-of cuts and federation
  exist. TerminusDB documents a single node.
- **Bus factor.** About 80% of TerminusDB's recent commits come from one
  maintainer. That is not a technical gap, but buyers ask.

## Gaps we could fill

Ranked by value to where Quipu is going: agent memory, governed knowledge
graphs, the merge paper, and the launch. Kinds: **(a)** feature,
**(b)** benchmark to adopt, **(c)** interop/import path,
**(d)** positioning/messaging.

### G1. Three-way fork promote: fix a silent revert we share with TerminusDB. (a), S–M

- **TerminusDB:** `/api/apply` merges from a caller-supplied `before_commit`.
  Their docs warn that passing the target tip "silently revert[s]" target-side
  changes and still reports success ([merge how-to][tdb-merge]).
- **Quipu today:** `Store::fork_promote` (`src/store/forks.rs:306`) computes
  `fork_diff("main", fork)`. That is a two-way set difference between ROOT's
  **current** facts and the fork's (`forks.rs:276-295`). The `removed` side is
  then retracted from ROOT. Any fact written to ROOT after the fork's anchor
  transaction is absent from the fork, so it lands in `removed` and gets
  retracted on promote, with no conflict and no report. This is the same
  failure TerminusDB documents. We built the correct operator for shares
  (`share_merge`) but do not use it for forks.
- **Proposal:** promote is a three-way merge with base = ROOT `as_of_tx =
  fork_tx` (the fork already records its anchor), ours = ROOT now, and theirs
  = the fork. Run it through `merge_graphs(base, ours, theirs, shapes)`. On
  conflict, return `DecisionRecord`s exactly as `quipu merge` does. Add a
  regression test: write to ROOT after `fork create`, then promote, and assert
  the write survives. Expose fork create, diff and promote on MCP, which
  `fork-at-any-event.md:140` already lists as follow-up.
- **Touches:** `src/store/forks.rs`, `src/share_merge.rs`, MCP definitions.
- **Why first:** it is a correctness bug in a shipped path. It is also the
  cleanest demonstration of the paper's claim inside the store itself rather
  than only across shares.

### G2. Agent-run branches as the flagship memory demo and benchmark. (b) + (d), M

- **TerminusDB:** their [agent page][tdb-agents] pitches "a branch per agent
  run, diff/merge review, commit history as temporal memory". It is the
  clearest competing story for our launch audience.
- **Quipu today:** we have every piece (forks, overlays, `quipu_merge_decisions`,
  bitemporal reads, episode retraction) but no single walkthrough or
  measurement that tells this story.
- **Proposal:** a reproducible benchmark-and-demo in `benchmark/`:
  1. N agents each work on a fork (or overlay) of a shared memory graph.
  2. Some make concurrent edits that conflict and some make edits that do
     not; some late-arriving corrections carry a past `valid_from`.
  3. Promote all of them, then measure: questions asked of a person
     (cardinality vs positional diff, reusing the paper's generator), silent
     losses (the G1 case as a negative control), and as-of answer correctness
     for back-dated corrections, which a commit-time-only store cannot
     express.

  Publish the numbers and run the TerminusDB arm through its documented
  workflow so the comparison is fair.
- **Depends on:** G1.

### G3. A standard patch format and an idempotent apply. (a) + (c), M

- **TerminusDB:** JSON diff and patch as a public, transportable API
  (`/api/diff`, `/api/patch`), with typed operations and `match_final_state`
  for idempotent re-application.
- **Quipu today:** four internal delta shapes: `fork_diff`, `share_diff`,
  `share_delta` (SPARQL Update) and `/changes` rows. None is a stable,
  documented interchange format outside shares. `standard-share-artifact.md:158-163`
  already chose parent-bound SPARQL Update as the share delta syntax, keeping
  RDF Patch and LD Patch for later media-type negotiation.
- **Proposal:** promote that choice to the store-wide change-set format:
  parent-bound SPARQL Update plus a small header carrying base tx, valid
  time, source and shapes-hash. Offer RDF Patch through content negotiation.
  Then:
  - `GET /diff?from=…&to=…` (forks, tx ranges, shares);
  - `POST /patch` that runs the write gate and is idempotent when the target
    already matches the post-state;
  - `share_delta` emits the same format.

  This is also the wire format G4 needs.
- **Touches:** `src/share_delta.rs`, `src/store/changes.rs`, a new route, and
  `docs/book/src/reference/rest-api.md`.

### G4. Push-back: governed instance-to-instance writes. (a), L

- **TerminusDB:** clone, fetch, push and pull between instances over HTTP.
- **Quipu today:** the pull side is shipped and stronger (followers with epoch
  and digest checks, relay chains, as-of cuts). Push-back (a signed changeset
  merged three-way at `base_tx`) exists only in `local-sync-mode.md`; no
  `base_tx` merge code exists in `src/`.
- **Proposal:** implement the local-sync push-back as designed:
  - the G3 patch, signed with the existing ed25519 plane;
  - merged at the receiver by the G1 three-way operator, against `base_tx`;
  - conflicts returned as decision records.

  The result is "git push for a governed graph", which TerminusDB cannot
  offer: it has push but no semantic conflicts, signatures or valid time.
- **Depends on:** G1 and G3.

### G5. Data-migration operations for accepted schema changes. (a), M

- **TerminusDB:** typed migration operations (CreateClass, MoveClass,
  MoveClassProperty, CastClassProperty, ChangeKey and others) applied
  atomically with the schema change, plus automatic migration
  ([reference][tdb-migr]).
- **Quipu today:** shapes and ontologies are bitemporally versioned
  (`shape-versioning.md`, implemented). Schema changes go through
  `quipu_propose_schema_change` and accept/reject (`src/proposal.rs`).
  Accepting records a decision; it does not rewrite instance data. Renaming a
  property or moving a class therefore leaves old facts that the new shapes
  refuse or ignore.
- **Proposal:** let a proposal carry a small migration program: rename
  predicate, retype class, cast literal datatype, split or merge property. The
  operations are expressed as SPARQL Update templates. On accept, the program
  runs through the write gate as one transaction with source
  `migration:<proposal>`. Valid time is preserved: old facts are superseded,
  not rewritten. Add a dry-run that reports which facts the new Σ would refuse
  before acceptance.
- **Touches:** `src/proposal.rs`, `src/mcp/proposal.rs`, the update gate.

### G6. TerminusDB import path: schema → SHACL, triples → episode. (c), M

- **TerminusDB:** the community build exports each graph as Turtle
  (`GET /api/triples/<path>`). The schema graph is itself triples in their
  `sys:` vocabulary ([`routes.pl:376-438`][tdb-routes]).
- **Quipu today:** we import any RDF, but nothing translates a TerminusDB
  schema into shapes.
- **Proposal:** a `quipu import terminusdb` (or a script under `scripts/`)
  that maps:
  - Class → `sh:NodeShape` with `sh:targetClass`;
  - Optional, Set and cardinality → `sh:minCount`/`sh:maxCount`;
  - Enum → `sh:in`;
  - `@inherits` → `rdfs:subClassOf`;
  - `@key` Lexical fields → a documented IRI convention.

  It loads instance triples as one episode whose source names the origin
  commit. Optionally it replays `/api/log` as a transaction history, so
  commit time becomes `tx` and valid time starts at commit time.
- **Why:** the most likely switchers are TerminusDB users who need valid time,
  value constraints or SPARQL. A one-command path costs little and is a
  credible launch line.
- **Note:** this needs nothing from TerminusDB beyond its open-build endpoints.

### G7. A composable query builder, not WOQL. (a) + (d), S–M

This section is the WOQL verdict.

**What WOQL gives its users that SPARQL text does not:**

- Queries are **data** (JSON-LD documents), so programs build, compose,
  store and transform them without string templating.
- Fluent builders in JS and Python.
- Recursion through tabled named queries (`Call`; `woql_compile.pl:3046`).
- Writes and reads in one query.

Everything else WOQL does is SPARQL 1.1: regular path queries
(property paths), Optional/Not/GroupBy, aggregation, and string, math and date
functions. Its graph selection (`From`/`Using`) is weaker than SPARQL's
dataset model. WOQL also has no standard and no second implementation.

**What Quipu already covers:**

- Recursion and Datalog: `rule_query` over the datafrog reasoner, with
  arbitrary body length and stratified negation, is more than WOQL's tabled
  recursion offers.
- Stored, validated, versioned parameterized queries: `src/store/queries.rs`
  and `quipu_queries`. These are WOQL's named-query feature with SPARQL
  inside.
- Queries as data: spargebra already parses SPARQL into an algebra tree on the
  server.

**Verdict:**

1. **Do not** implement WOQL or a WOQL→SPARQL translator. Its user base is
   small, it is one maintainer's dialect, and translating it would put a
   second, non-standard query surface in front of the write gate.
2. **Do** ship a small typed SPARQL builder in the Python client
   (`python/quipu_client`) and the WASM/JS binding. It needs triple and path
   patterns, optional, union, filter, group/aggregate, `valid_at`/`as_of_tx`,
   and serialization to SPARQL text. It also needs a "save as stored query"
   call, so composition ends in the governed `queries` table. This gives
   agents and notebooks WOQL's composability while every query stays standard
   SPARQL that any RDF tool can read.
3. Messaging: "Datalog where you want rules, SPARQL where you want queries,
   both over the same bitemporal facts."

**Size:** S for Python, M once the JS binding is included.

### G8. Agent-facing docs: `llms.txt` and markdown pages. (d), S

- **TerminusDB:** publishes `llms.txt` and `llms-full.txt`, serves every docs
  page as `/page.md`, and has a paste-in prompt for coding agents in the
  quickstart.
- **Quipu today:** the mdBook has none of these (`find . -name 'llms*.txt'`
  finds nothing).
- **Proposal:** generate `llms.txt` and `llms-full.txt` from `docs/book/src`
  in the docs workflow, and add an agent quickstart prompt. This is cheap and
  aimed directly at the launch audience.

### G9. Deterministic IRI minting strategies. (a), S–M; watch

- **TerminusDB:** `@key` Lexical, Hash, ValueHash and UuidV7 give
  deterministic document IRIs from field values, so two writers creating "the
  same" thing converge on one IRI.
- **Quipu today:** stable entity references are designed
  (`docs/design/stable-entity-references.md`) and entity resolution exists.
  The merge paper's own limit is 0/93 alias repairs: different names for one
  entity stay invisible to the merge.
- **Proposal:** a shape-level annotation (for example
  `quipu:keyFields (ex:host ex:port)`) that mints a lexical or hash IRI at
  write time. That prevents aliases instead of repairing them afterward.
- **Status:** worth building only after `stable-entity-references.md`
  settles, because it must not contradict "an IRI identifies the thing
  independently of a label".

## Not worth copying

- **Squash and optimize of history.** TerminusDB compacts delta layers and can
  squash commits because its history is storage. Quipu's log is the record,
  and squashing would destroy the audit and valid-time answers that
  differentiate us. Freeze and deep-freeze already move cold history into
  verified read-only packs, which is the governed version of the same storage
  need.
- **Rebase.** Rebase rewrites history so it reads linearly. In a bitemporal
  store, history is evidence. Three-way merge with recorded decisions (G1)
  gives the same outcome without rewriting anything.
- **WOQL itself.** See G7.
- **Auto-generated GraphQL.** TerminusDB generates it from a closed class
  schema where every property has one type. Generating it from open-world
  SHACL plus RDFS means picking a projection per shape, and someone has to
  maintain that projection. MCP is the agent surface and SPARQL is the
  developer surface; a third surface splits attention before launch. Revisit
  only if a UI customer asks.
- **Document-first JSON API as the primary model.** Their JSON documents
  convert losslessly to triples only because the schema is closed. Quipu's
  episode and knot inputs already accept structured nodes and edges for
  agents, which covers the ergonomic need without a second canonical model.
- **Single-store commit queue and succinct storage.** These answer TerminusDB's
  in-memory architecture. Quipu's SQLite log with read pools and followers
  addresses a different scaling shape. Our measured bottleneck is ingest on
  real disks (about 1k facts/s on ext4), and succinct layers would not fix
  that.

## Positioning note

TerminusDB is a versioned document database. It treats a graph like a git
repository of JSON documents: every commit is a snapshot, and history means
when the database changed. Quipu is a governed knowledge graph. It records
when each fact was true and when we learned it, refuses writes that break
SHACL shapes, and decides merge conflicts from those same shapes, so two
agents adding to a multi-valued property never interrupt a person and two
agents disagreeing on a functional property always do. TerminusDB is the
better fit when you want git-style workflows over JSON with a GraphQL API.
Quipu is the better fit when you need standard RDF and SPARQL, valid time,
constraint enforcement and provenance that hold up in an audit, which is what
long-lived agent memory needs.

## Suggested next steps

1. File G1 now as a correctness bug with the regression test described.
2. File G3 → G4 as one dependent chain, since the patch format is the wire
   format for push-back.
3. Fold G2 into the launch plan as the agent-memory demo, after G1 lands.
4. File G5–G8 as normal-priority items. Keep G9 on watch until stable entity
   references settle.
5. Update the TerminusDB row in `docs/design/vision.md` the next time that
   file is edited.

[tdb-repo]: https://github.com/terminusdb/terminusdb
[tdb-commit]: https://github.com/terminusdb/terminusdb/tree/0b5549acdd6e5d1d34564d728cf58fa677358a1e
[tdb-docs]: https://terminusdb.com/docs/
[tdb-llms]: https://terminusdb.com/llms-full.txt
[tdb-cargo]: https://github.com/terminusdb/terminusdb/blob/0b5549acdd6e5d1d34564d728cf58fa677358a1e/src/rust/Cargo.toml
[tdb-jsonrdf]: https://github.com/terminusdb/terminusdb/blob/0b5549acdd6e5d1d34564d728cf58fa677358a1e/src/core/document/json_rdf.pl
[tdb-routes]: https://github.com/terminusdb/terminusdb/blob/0b5549acdd6e5d1d34564d728cf58fa677358a1e/src/server/routes.pl
[tdb-apidoc]: https://github.com/terminusdb/terminusdb/blob/0b5549acdd6e5d1d34564d728cf58fa677358a1e/src/core/api/api_document.pl
[tdb-rollup]: https://terminusdb.com/docs/delta-rollup/
[tdb-ref]: https://github.com/terminusdb/terminusdb/blob/0b5549acdd6e5d1d34564d728cf58fa677358a1e/src/terminus-schema/ref.json
[tdb-json]: https://github.com/terminusdb/terminusdb/blob/0b5549acdd6e5d1d34564d728cf58fa677358a1e/src/core/document/json.pl
[tdb-schema]: https://terminusdb.com/docs/schema-reference-guide/
[tdb-restr]: https://github.com/terminusdb/terminusdb/blob/0b5549acdd6e5d1d34564d728cf58fa677358a1e/src/core/query/restrictions.pl
[tdb-migr]: https://terminusdb.com/docs/schema-migration-reference-guide/
[tdb-patch]: https://github.com/terminusdb/terminusdb/blob/0b5549acdd6e5d1d34564d728cf58fa677358a1e/src/core/document/patch.pl
[tdb-apipatch]: https://github.com/terminusdb/terminusdb/blob/0b5549acdd6e5d1d34564d728cf58fa677358a1e/src/core/api/api_patch.pl
[tdb-apply]: https://github.com/terminusdb/terminusdb/blob/0b5549acdd6e5d1d34564d728cf58fa677358a1e/src/core/document/apply.pl
[tdb-merge]: https://terminusdb.com/docs/merge-howto/
[tdb-rebase]: https://github.com/terminusdb/terminusdb/blob/0b5549acdd6e5d1d34564d728cf58fa677358a1e/src/core/api/db_rebase.pl
[tdb-woql]: https://github.com/terminusdb/terminusdb/blob/0b5549acdd6e5d1d34564d728cf58fa677358a1e/src/terminus-schema/woql.json
[tdb-woqlc]: https://github.com/terminusdb/terminusdb/blob/0b5549acdd6e5d1d34564d728cf58fa677358a1e/src/core/query/woql_compile.pl
[tdb-gql]: https://github.com/terminusdb/terminusdb/tree/0b5549acdd6e5d1d34564d728cf58fa677358a1e/src/rust/terminusdb-community/src/graphql
[tdb-vec]: https://github.com/terminusdb/terminusdb/tree/0b5549acdd6e5d1d34564d728cf58fa677358a1e/vectorsearch
[tdb-vsearch]: https://terminusdb.com/docs/versioned-search/
[tdb-sys]: https://github.com/terminusdb/terminusdb/blob/0b5549acdd6e5d1d34564d728cf58fa677358a1e/src/terminus-schema/system_schema.json
[tdb-agents]: https://terminusdb.com/docs/knowledge-graph-for-ai-agents/
[tdb-cloud]: https://terminusdb.com/docs/cloud/
