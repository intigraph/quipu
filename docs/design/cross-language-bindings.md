# Cross-language Quipu: WASM parity, then Python

Status: design and measured baseline for aegis-gqo (2026-10-19). This supersedes
an earlier hooks/plugins proposal. The deliverable is the existing Rust engine
usable as a real governed store from JS and Python, not a new plugin host.
WASM comes first; Python follows its acceptance. No publication or production
activation is implied by this document.

## Measured baseline and scope

Pinned Quipu source: `a7510ca8ea23605b395f164b8bd56dd3de7c26ac`.
Pinned Creel source: `f31b65be440378955f8a456360c237b16dbeb8b3`.
The accompanying [machine-readable inventory](binding-parity.json) lists every
one of the **79 tool-name literals** in `src/mcp/definitions{,/**}.rs` individually.
This is a feature union, not proof all names compile on each target. Native
library lifecycle and typed operations are additional rows below; the tool list
is not a claim to enumerate every public Rust helper or trait method.

WASM is already writable. `wasm/explorer/src/lib.rs` calls native episode, set,
retract and query handlers; `features.rs` wires SHACL feedback, OWL and reactive
reasoners. The explorer defaults to those features and validates local writes
after adopting pack shapes. It is currently constructed from a pack into a
memory store, not a general durable store constructor.

Creel has a second, independent wrapper in `wasm/quipu-provider/src/lib.rs`:
38 manual dispatch arms, OPFS/open/serialize/history functions, and a Quipu
dependency with `default-features = false`. Its shipped host catches OPFS failure
and falls back to memory. The browser can therefore write without establishing
durable or governed parity. Do not treat its source comment promising complete
MCP parity as evidence: **41 current native names have no dispatch arm**.

Actual existing Creel `node tests/test-quipu.js` baseline: **9 passed**, Chromium
availability positively checked, test HOME guard **158 paths + 5 resources clean**.
This includes episode write/read, multi-tab sharing, offline boot and host death.
It uses Creel's already-vendored WASM: its binding to the current Quipu revision
is UNKNOWN. These nine checks are useful positive controls, not the new
SHACL/governance/durable-current-engine acceptance. An unavailable browser must
fail this project's acceptance instead of using that test's successful skip.

`python/quipu_client` is a zero-dependency setuptools REST client. It stays that
way. No in-process Python binding or wheel is present at this baseline.

## One engine and one capability contract

Introduce a Rust library facade (`bindings::Engine` and a versioned capability
manifest), not a dispatch table in each language. It owns a `Store` and typed
request context. Every operation delegates to the existing Rust operation,
including its transaction, vocabulary, SHACL, governance, labels and inference
rules. No adapter may write SQL directly or reproduce validation in JS/Python.
The manifest and dispatch definitions must have one source of truth: CI rejects
an advertised operation without a handler or an added native operation without
a parity disposition and test. Do not expose server transport handlers by
blindly forwarding them: some require host services or verified request context.

Capabilities report the exact Rust revision, contract version, compiled engine
features, storage mode, schema/shape adoption state and operation availability.
A disabled feature returns a structured unsupported-capability error; it never
silently weakens validation. That error documents an OPEN parity gap, not a way
to declare full parity. Shape-governed writes require explicit adopted shapes.
A caller-supplied actor is provenance, not authenticated principal authority.
Signed/verified server claims must not be forged by a local binding.

Keep typed Rust APIs available; a common JSON command layer makes transport and
conformance adapters small, while Python convenience methods expose Python
objects. JSON text preserves native number lexemes; convenience JS objects must
use lossless ID/transaction/timestamp handling (BigInt or decimal strings with
explicit conversion), not round-trip arbitrary 64-bit IDs through Number.
The `entityHistory` worker relay returns safe integers as `Number` and unsafe
integer tokens as `BigInt`, preserved through worker structured cloning. It uses
the [JSON reviver's original source text](https://tc39.es/ecma262/multipage/structured-data.html#sec-json.parse)
and refuses unsafe integers when that parser capability is unavailable. Strings
and floating-point values retain their native JSON types. Callers serializing a
returned `BigInt` to JSON must choose an explicit lossless representation.
Preserve IRI/literal/datatype/language/blank-node identity and native error data.
Errors carry stable code, operation, commit-state (`not_started`, `committed`, or
`unknown`), validation/governance details and retryability. A thrown exception or
timeout never implies the transaction did not commit. No automatic write retry.
Raw low-level store mutation APIs do not become a second public bypass API.

## Native API / WASM parity matrix

### Incremental shared engine facade

`quipu::bindings::Engine::open_in_memory()` creates a transport-free local
store. `invoke(operation, &serde_json::Value)` and `invoke_json(operation,
json_text)` dispatch through one Rust registry. Its initial eight operations
are query, export, entity history, knot, episode, set, retract and shapes, using
their exact `quipu_*` names and existing native handlers. Inspect
`capabilities()` for this build's availability and current shape catalogue.
Writes require a SHACL-enabled build and explicitly adopted shapes. No reasoner
is registered by this constructor, and it reports that limitation.
`schema_state` and each operation's `available` field reflect the current
catalogue. Empty or prefix-only catalogues refuse data writes before dispatch;
removing the last class vocabulary disables data writes again. This is an
explicit binding precondition; native library bootstrap behavior is unchanged.

The WASM bundle additionally exports `Database`, an empty memory-store
constructor backed by that same facade. This is separate from the compatible
pack-based `Explorer`. For example:

```javascript
const db = new Database();
const caps = JSON.parse(db.capabilities());
// Adopt application shapes explicitly with quipu_shapes before writing.
const raw = db.invoke("quipu_query", JSON.stringify({query: "SELECT ?s WHERE { ?s ?p ?o }"}));
db.free();
```

`invoke` returns JSON **text**. Keep it as text or apply a lossless parser when
the result can contain native integers beyond JavaScript's safe range. A thrown
error is JSON text with `code`, `operation`, `commit_state`, `retryable`,
`message` and native `details` where available. Binding refusals report
`not_started`; errors after native dispatch conservatively report `unknown`.
Neither error nor timeout authorizes an automatic write retry. Rust callers
can also inspect the original native error through `Error::source`.

This facade is incremental: it reports `full_parity: false`. It does not yet
provide OPFS lifecycle, all native operations, a Creel migration, or Python.
The matrix below retains the original pinned baseline rather than promoting
these initial delegates to full acceptance. Build the additive bundle without
fetching models or regenerating the book pack with `just wasm explorer-build`.

In the following per-tool rows, “present” means source routing is present, not
behavioral acceptance. All rows need the shared conformance suite at the exact
built revision. Python native bindings are missing for every row today.

| Native tool | Current explorer source | Current Creel source | Required work |
| --- | --- | --- | --- |
| `quipu_query` | partial: query(str); native JSON query options not all exposed | manual arm present | shared engine dispatcher + conformance |
| `quipu_export` | partial: exportNtriples/exportPack/manifest/delta, not full native input contract | manual arm present | shared engine dispatcher + conformance |
| `quipu_align_propose` | not exposed | missing arm | shared engine dispatcher + conformance |
| `quipu_align_decide` | not exposed | missing arm | shared engine dispatcher + conformance |
| `quipu_align_apply` | not exposed | missing arm | shared engine dispatcher + conformance |
| `quipu_merge_decisions` | not exposed | missing arm | host adapter required; acceptance OPEN |
| `quipu_merge_apply` | not exposed | missing arm | host adapter required; acceptance OPEN |
| `quipu_knot` | not exposed | manual arm present | shared engine dispatcher + conformance |
| `quipu_cord` | not exposed | manual arm present | shared engine dispatcher + conformance |
| `quipu_unravel` | not exposed | manual arm present | shared engine dispatcher + conformance |
| `quipu_validate` | partial: validateShapes; standalone validation, not entire tool contract | manual arm present | shared engine dispatcher + conformance |
| `quipu_shapes` | not exposed | manual arm present | shared engine dispatcher + conformance |
| `quipu_queries` | not exposed | manual arm present | shared engine dispatcher + conformance |
| `quipu_graph_create` | not exposed | missing arm | shared engine dispatcher + conformance |
| `quipu_graph_label` | not exposed | missing arm | shared engine dispatcher + conformance |
| `quipu_graph_list` | not exposed | missing arm | shared engine dispatcher + conformance |
| `quipu_graph_freeze` | not exposed | missing arm | shared engine dispatcher + conformance |
| `quipu_graph_thaw` | not exposed | missing arm | shared engine dispatcher + conformance |
| `quipu_datasets` | not exposed | manual arm present | shared engine dispatcher + conformance |
| `quipu_retract` | native delegate present | manual arm present | shared engine dispatcher + conformance |
| `quipu_set` | native delegate present | manual arm present | shared engine dispatcher + conformance |
| `quipu_retract_episode` | not exposed | manual arm present | shared engine dispatcher + conformance |
| `quipu_retract_source` | not exposed | missing arm | shared engine dispatcher + conformance |
| `quipu_episode` | native delegate present | manual arm present | shared engine dispatcher + conformance |
| `quipu_entity` | not exposed | missing arm | shared engine dispatcher + conformance |
| `quipu_entity_history` | not exposed | missing arm | shared engine dispatcher + conformance |
| `quipu_transactions` | not exposed | missing arm | shared engine dispatcher + conformance |
| `quipu_embed_backfill` | not exposed | missing arm | shared engine dispatcher + conformance |
| `quipu_embed_backfill_graph` | not exposed | missing arm | shared engine dispatcher + conformance |
| `quipu_search_query` | not exposed | missing arm | shared engine dispatcher + conformance |
| `quipu_fragments` | not exposed | missing arm | shared engine dispatcher + conformance |
| `quipu_reconcile` | not exposed | missing arm | host adapter required; acceptance OPEN |
| `quipu_spotlight` | not exposed | missing arm | shared engine dispatcher + conformance |
| `quipu_stats` | not exposed | missing arm | shared engine dispatcher + conformance |
| `quipu_health` | not exposed | missing arm | runtime adapter required; acceptance OPEN |
| `quipu_version` | not exposed | missing arm | runtime adapter required; acceptance OPEN |
| `quipu_share` | partial: exportPack with explicit receiver sharing checks | missing arm | shared engine dispatcher + conformance |
| `quipu_import` | partial: loadPendant fixed import+promotion ceremony | missing arm | shared engine dispatcher + conformance |
| `quipu_import_promote` | partial: loadPendant internal promotion; no general explicit API | missing arm | shared engine dispatcher + conformance |
| `quipu_acquire` | not exposed | missing arm | host adapter required; acceptance OPEN |
| `quipu_acquire_composed` | not exposed | missing arm | host adapter required; acceptance OPEN |
| `quipu_knot_stage` | not exposed | missing arm | shared engine dispatcher + conformance |
| `quipu_knot_promote` | not exposed | missing arm | shared engine dispatcher + conformance |
| `quipu_reason` | not exposed | missing arm | shared engine dispatcher + conformance |
| `quipu_update` | not exposed | missing arm | shared engine dispatcher + conformance |
| `quipu_events` | not exposed | missing arm | runtime adapter required; acceptance OPEN |
| `quipu_events_commit` | not exposed | missing arm | runtime adapter required; acceptance OPEN |
| `quipu_changes` | not exposed | missing arm | runtime adapter required; acceptance OPEN |
| `quipu_subscriptions` | not exposed | missing arm | runtime adapter required; acceptance OPEN |
| `quipu_workflow_capabilities` | not exposed | missing arm | runtime adapter required; acceptance OPEN |
| `quipu_policy_check` | not exposed | manual arm present | shared engine dispatcher + conformance |
| `quipu_verdict_verify` | not exposed | manual arm present | shared engine dispatcher + conformance |
| `quipu_verifier_authorized` | not exposed | manual arm present | shared engine dispatcher + conformance |
| `quipu_cooccurrence` | not exposed | manual arm present | shared engine dispatcher + conformance |
| `quipu_overlay_create` | not exposed | manual arm present | shared engine dispatcher + conformance |
| `quipu_overlay_write` | not exposed | manual arm present | shared engine dispatcher + conformance |
| `quipu_overlay_compose` | not exposed | manual arm present | shared engine dispatcher + conformance |
| `quipu_resolve_entity` | not exposed | manual arm present | shared engine dispatcher + conformance |
| `quipu_graph` | not exposed | manual arm present | shared engine dispatcher + conformance |
| `quipu_project` | not exposed | manual arm present | shared engine dispatcher + conformance |
| `quipu_context` | not exposed | manual arm present | shared engine dispatcher + conformance |
| `quipu_report` | native delegate present | manual arm present | shared engine dispatcher + conformance |
| `quipu_rule_query` | not exposed | missing arm | shared engine dispatcher + conformance |
| `quipu_ask` | not exposed | manual arm present | shared engine dispatcher + conformance |
| `quipu_propose_schema_change` | not exposed | manual arm present | shared engine dispatcher + conformance |
| `quipu_list_proposals` | not exposed | manual arm present | shared engine dispatcher + conformance |
| `quipu_accept_proposal` | not exposed | manual arm present | shared engine dispatcher + conformance |
| `quipu_reject_proposal` | not exposed | manual arm present | shared engine dispatcher + conformance |
| `quipu_search` | not exposed | manual arm present | shared engine dispatcher + conformance |
| `quipu_hybrid_search` | not exposed | manual arm present | shared engine dispatcher + conformance |
| `quipu_search_nodes` | not exposed | manual arm present | shared engine dispatcher + conformance |
| `quipu_search_facts` | not exposed | manual arm present | shared engine dispatcher + conformance |
| `quipu_episodes_complete` | not exposed | manual arm present | shared engine dispatcher + conformance |
| `quipu_impact` | not exposed | manual arm present | shared engine dispatcher + conformance |
| `quipu_path_cone` | not exposed | missing arm | shared engine dispatcher + conformance |
| `quipu_path_backtest` | not exposed | missing arm | shared engine dispatcher + conformance |
| `quipu_unified_search` | not exposed | manual arm present | shared engine dispatcher + conformance |
| `quipu_load_ontology` | native delegate present | manual arm present | shared engine dispatcher + conformance |
| `quipu_explain` | not exposed | missing arm | shared engine dispatcher + conformance |

### Library lifecycle and typed API families

These are obligations in addition to the tool rows, not aliases that remove
work from them. During implementation, pin a rustdoc/public-symbol inventory for
each exported family; changes fail the inventory check until classified. Public
helper declarations are not all independently callable external operations.

| Native library surface | WASM baseline | Python baseline | Required acceptance |
| --- | --- | --- | --- |
| `Store::open_in_memory` | explorer pack loader; Creel open_memory | absent | empty and shaped stores, explicit feature/config defaults |
| `Store::open(path)` / close | harness + Creel OPFS, not general explorer | absent | worker-only durable open, explicit close, same DB after reload/relaunch |
| `Store::open_from_bytes` / `serialize_db` | Creel + harness; no general explorer handle | absent | native → browser → native, history/config/shapes retained |
| Config and namespace | mostly fixed explorer defaults | absent | typed options, validation, exact effective config returned |
| Shapes load/list/remove/vocabulary | implicit pack adoption; partial standalone validation | absent | explicit receiver adoption, immediate write enforcement, rejected-state absence |
| Episode ingest/prepare/commit/resolution | episode wrapper; no complete prepare/commit surface | absent | atomic success and refusals, provenance and type admission unchanged |
| RDF knot/load/replace/delete graph | missing general explorer methods | absent | formats/terms and named scopes, transaction rollback on refusal |
| Set/retract/source/episode retraction | partial explorer methods | absent | bitemporal facts close rather than disappear; same source provenance |
| SPARQL query/temporal query/provider API | query text + browser provider adapters | absent | same SELECT/ASK/CONSTRUCT/temporal answers and limits |
| Entity/fact/transaction history | Creel custom entity serializer | absent | native history DTO, valid-time vs transaction-time, as-of reads |
| Governance precommit / labels / policies | core linked, coverage unproved in Creel feature build | absent | allow/deny/unknown, no partial writes, no attribution escalation |
| OWL / Datalog / observers | explorer features + reasoner wiring | absent | same derived graph, retractions, explicit ontology authority |
| Graph projection / PageRank / Louvain / graph views | native Rust compiles; general exporter gap | absent | same deterministic partitions, ranks, bounds and scope |
| Search / embedding / context / retrieval | JS runtime adapters + native query/search subset | absent | no remote fallback hidden as local parity; same declared provider semantics |
| Sharing/import/stage/promote/delta | fixed pack loading/export ceremony | absent | byte integrity, parent/hash binding, quarantine, explicit promotion, graph scope |
| Reconcile / filesystem acquisition / git merge | native host services | absent | explicit byte/host adapter; no browser filesystem or shell emulation |
| Fork/snapshot/read pins / speculate / impact | no complete public lifecycle adapter | absent | isolated fork, rollback/commit boundaries and stable snapshots |
| Vector backend/provider interfaces | browser JS MiniLM; no native ONNX in WASM | absent | provider contract and capabilities, explicit backend-specific differences |

“Full parity” means engine semantics and a usable equivalent for supported host
operations. A browser cannot spawn a shell or serve HTTP. Native-only process
operations must have explicit app-owned adapters or remain an OPEN acceptance
row; they are never silently omitted from the contract. This work does not
promise that every platform can run every native deployment process.

## Browser architecture and durability

Keep the engine in a dedicated worker; OPFS synchronous handles cannot run on
the UI thread. Promote the existing harness/Creel OPFS integration into the
maintained Quipu binding rather than inventing a storage backend. Expose
`openMemory`, asynchronous VFS initialization plus `openPersistent`, `close`,
byte snapshot import/export and explicit durability status. Persistent open
must refuse when unavailable; the caller can separately request memory mode.
Memory fallback must not report a successful durable-store open.

Use one writer per database. Creel already elects a host and forwards calls;
retain that application ownership boundary. The library reports busy/lock,
quota, permission and storage-eviction errors instead of trying to elect tabs.
Release handles on close; a second writer must fail or obey explicit arbitration.
A cancelled request may already have committed: replies and replay checks must
use transaction/operation receipts, not assume cancellation rolls it back.

OPFS is origin/profile scoped, not a shared host filesystem. Tests reuse the
same origin and profile on reload and relaunch and reject a memory VFS. SQLite's
SAH-pool and other OPFS VFSes do not share names or files merely because a path
string matches. Persistence also needs browser storage permission/quota checks;
it is not a guarantee against user deletion or browser eviction.

`Explorer` remains compatible for pack-centric consumers. An additive general
engine handle backs both it and Creel; the pack-only constructor must not be
silently changed into a different authority/adoption ceremony. Imports and sync
use the existing native share/delta/canonical-hash and quarantine/promote logic.
A downloaded pack supplies evidence, not its own promotion authority. No sync
transport, network credential or private catalogue is embedded in a package.

## Python architecture

Add a separate native extension crate (proposed `bindings/python`) using PyO3
and maturin, linked to this Rust core by a path dependency in source builds.
Keep `python/quipu_client` remote and separately installable. The extension's
public import is proposed as `quipu`, with `Store.open(path)`, `Store.memory()`,
context-manager close, shapes/episode/knot/query/history methods, and access to
the versioned complete command/capability contract. Return Python dict/list,
int, bytes and typed RDF/error objects instead of encoded JSON strings from
convenience methods; preserve a low-level JSON command entry point for exact
fixtures. No HTTP process or sidecar is needed for local calls.

Initially enforce handle thread affinity and non-reentrant calls explicitly;
do not claim that the GIL alone makes Rust state safe. PyO3 thread-safety rules
must be satisfied by the actual `Store` ownership model. `unsendable` is an
explicit limitation with cross-thread refusal tests, not a concurrency promise.
Releasing the GIL or supporting free-threaded CPython is a later measured
capability requiring proven Send/Sync/lock behavior and no Python callbacks
under a store transaction. Python extension code never runs arbitrary Python
as a pre-write hook. Stable ABI (`abi3`) is a packaging decision after testing
feature/API support, not an assumption that removes interpreter coverage.

The requested `pip install quipu` currently names an unrelated expression
Evaluator on PyPI (version 0.1, released 2012). Do not claim ownership or overwrite
it. The approved Python distribution name is `quipu-kg` (`pip install quipu-kg`);
the import name remains `quipu`. This supersedes the earlier illustrative
`quipu-ai` suggestion. Registry ownership and the exact publisher identity
remain unverified; first publication needs the existing one-way release gate.
Local wheel testing follows the WASM acceptance milestone.

First slice (aegis-gqo.1): `bindings/python` builds `quipu-kg` with PyO3 and
maturin over `bindings::Engine`, which now also opens file-backed stores
(`Engine::open(path)`, reported as `storage: "file"`). It exposes
`Store.memory()`/`Store.open(path)`, context-manager close, shapes, knot,
episode, set, query (current, `as_of_tx`, `valid_at`) and history, plus the
low-level `invoke`/`invoke_json` entry points and `capabilities()`. A refused
knot raises `quipu.ValidationError` with the native SHACL report. Handles
refuse calls from any thread but the opener with `ThreadAffinityError`
(`not_started`); the GIL is held for every call. Query results return IRIs and
plain strings both as `str` because the native JSON does not distinguish them;
typed IRI results stay an OPEN fidelity row. An sdist built outside a git
checkout reports revision `unknown` (honest, not pinned); pinning the revision
into the sdist is OPEN. The shared corpus is
`tests/fixtures/bindings-conformance/v1.json`, run natively by
`tests/bindings_conformance.rs` and in Python by `bindings/python/tests`.
The wheel/sdist workflow (no publish job) is staged at
`bindings/python/ci/python-bindings.yml` until a workflow-scoped push installs
it under `.github/workflows/`.

## Packaging and release

Pin the Rust revision and feature set in every WASM/npm artifact, wheel and
sdist. Align the binding release version with the Rust core/release-plz version
and include a machine-readable engine/contract identity in the installed API.
Do not let release-plz's root version bump leave a nested Cargo.lock stale;
validate all binding manifest/lock identities before artifact creation.

WASM keeps both browser ESM and Node bindings built from the **same `.wasm`**;
smoke the packaged layouts, not only workspace directories. Publish generated
TypeScript declarations and runtime adapters together, with hashes/provenance.
A package manifest advertising a feature absent from the linked binary fails CI.
Creel pins a released artifact/digest or exact source dependency, not floating
main plus an unbound copied WASM file. Preserve existing pack-centric consumers.

Python builds macOS arm64/x86_64 and Linux x86_64/aarch64 wheels, plus an sdist
that successfully builds its wheel. Use maturin's platform auditing and install
those wheel files into clean environments with the native repository/server
unavailable. Test the claimed CPython versions; initially 3.11–3.14 conventional
builds, with free-threaded support only when separately proven. Windows is an
OPEN packaging row until explicitly added, not advertised as supported.

Use an exact owner-configured PyPI Trusted Publisher/OIDC identity for the
reviewed repository/workflow/environment. PyPI supports pending publishers for
new projects; unlike the retained crates.io incident, a manual bootstrap upload
is not inherently required. Configuring a pending publisher does not reserve a
name. No token creation or registry publication is part of a source merge.
Keep wheel building separate from the irreversible publish job. Source changes
follow the current public PR/CI/one independent post-green review policy; Wu
retains actual Quipu merge/CD authority until an explicit transfer.

## Shared conformance and the acceptance oracle

One versioned fixture corpus drives native, WASM-browser, WASM-Node and Python
adapters. Every case records shapes/config, actor/context, inputs, expected
result/error code, visible facts, transaction state and temporal history. Compare
canonical RDF terms and graph scope, not JSON object ordering or incidental
wall-clock transaction IDs. Fix timestamps in fixtures where the native API
supports them; report genuinely variable metadata separately. Do not normalize
away differing datatypes, language tags, errors or rejected writes.

Required cases include:

1. Empty store, explicit shape adoption, valid episode and knot writes, query,
   update/retract and entity/transaction history. Same facts and native guards.
2. Missing/off-vocabulary type, SHACL violation, governance deny and unknown:
   structured refusal **and** direct state/history assertions that no rejected
   fact or partial transaction leaked. Each refusal has a valid positive control.
3. Data-graph subclasses, named-graph isolation, labels, reactive retraction,
   temporal as-of reads, imported graph quarantine and explicit promotion.
4. All registered operations are either callable and tested or explicitly
   unavailable with a reason; unavailable core operations keep parity OPEN.
   Inject a missing handler in the registry coverage test and make it fail.
5. Native DB bytes → browser → native and native shares → browser → export /
   delta → fresh native receiver, preserving authority and hash/parent checks.
6. Actual Creel page and worker (not a hand-written mock): use the newly built
   Quipu binding, assert Rust revision/features, load fixture shapes/policies,
   perform valid and refused episode/knot writes, read query/history, close and
   reload/relaunch the same browser profile/origin, and verify persisted state.
   Check the worker really uses OPFS, not Creel's current catch-all RAM fallback.
7. Second writer contention, host death during a request, quota/persistence
   refusal and malformed imports. No automatic duplicate write replay. Ensure
   a browser or package import failure cannot become a successful skipped test.
8. Wheel/sdist installed-artifact tests use the identical fixture corpus, with
   large integer/RDF fidelity, closed-handle and cross-thread refusal controls.

Existing Creel's nine tests and Quipu's explorer browser/Node/SHACL smoke tests
remain positive regression controls. They do not replace the new current-core
Creel OPFS/governed-write acceptance. Feature tests should not require live
production services, private graphs, model hubs or credentials. Use local
synthetic stores/archives. Browser-only provider/model behavior needs its
existing explicit optional provider capabilities, not a forced network fetch.

## Reviewable sequence and ownership

1. **Design/inventory (this document):** report to Hammond before the first
   build PR. Record exact source and existing browser baseline in Quipu.
2. **WASM core write surface:** expose the missing portable operations through
   shared Rust delegates (starting with knot, shapes and history), structured
   errors and capabilities; add source/registry drift and shared fixture tests.
   Keep the existing Explorer API compatible.
3. **WASM lifecycle/OPFS + Creel migration:** promote the real harness storage
   adapter, switch Creel off its duplicate dispatcher, and land the actual
   current-core governed-write/reload/relaunch test. Coordinate Creel-owned
   source separately; no edits to another author's checkout. No full-parity
   completion claim until every core row and the consumer acceptance pass.
4. **Remaining portable and host adapters:** complete named graph/history,
   query/provider/config, import/export/delta and required host operation rows;
   compare complete fixtures. Source-only CI green is not installed acceptance.
5. **Python:** after the WASM milestone, add PyO3 facade and Pythonic types,
   shared corpus runner, local wheel/sdist matrix; then prepare the exact naming
   and OIDC publication packet. Other languages follow only after measured
   core/binding contracts: JS/TS uses WASM; evaluate UniFFI or a narrow C ABI for
   Go/Swift/Kotlin without another policy engine or public raw-store ABI.

No WASM Component Model, Extism, Python plugin callbacks, custom SPARQL plugin
host or out-of-process extension service is required for these goals. Any later
extension proposal is independent work, with its own deterministic replay and
provenance design. This plan does not auto-unpark the historical browser pack
research item or authorize production imports, shape loads or a release.

## References

- Existing [WASM/storage design](wasm-support.md) and [Python REST client](../../python/README.md).
- [SQLite WASM persistence and VFS differences](https://www.sqlite.org/wasm/doc/trunk/persistence.md).
- [PyO3 thread safety](https://pyo3.rs/v0.29.3/class/thread-safety) and
  [free-threading](https://pyo3.rs/main/free-threading).
- [Maturin distribution/platform auditing](https://www.maturin.rs/distribution.html).
- [PyPI OIDC publishing](https://docs.pypi.org/trusted-publishers/) and
  [new-project pending publishers](https://docs.pypi.org/trusted-publishers/creating-a-project-through-oidc/).
- [Existing unrelated `quipu` project](https://pypi.org/project/quipu/).
