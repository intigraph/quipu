# Named graph surface parity audit

This audit covers the CLI dispatch in `src/main.rs`, the registered REST route
literals in `src/server.rs` and its router modules, and the native MCP manifest
and route mapping. It describes surface coverage, not proof that every option
on every command behaves identically. The fresh-store recipe proves the named
graph path through the actual native MCP transport.

## Named graph operations

| Operation | CLI | REST | MCP |
| --- | --- | --- | --- |
| Register committed graph | `graph create` | `POST /graph/create` | `quipu_graph_create` |
| Declare labels | `graph label --label JSON` | `POST /graph/label` | `quipu_graph_label` |
| List registry and creation provenance | `graph list --json` | `GET /graphs` | `quipu_graph_list` |
| Freeze / thaw | `graph freeze` / `graph thaw` | `POST /graph/freeze` / `/graph/thaw` | `quipu_graph_freeze` / `quipu_graph_thaw` |
| Create / write / compose overlay | `graph overlay-create` / `overlay-write` / `overlay-compose --input JSON` | `/overlay/create` / `/overlay/write` / `/overlay/compose` | `quipu_overlay_create` / `quipu_overlay_write` / `quipu_overlay_compose` |
| Load vocabulary / write / query | `shapes load` / `knot --graph` / `query --graph` | `/shapes` / `/knot` / `/query` | `quipu_shapes` / `quipu_knot` / `quipu_query` |
| Import local database file | `graph import file --as IRI` | Portable share `/import` then `/import/promote` | `quipu_share` / `quipu_import` / `quipu_import_promote` |

The create/label gaps are closed by sharing existing handlers, not copying
registration logic into each transport. Actor is caller-declared creation
provenance; it never establishes authorization. Local-file imports are not
remote file-path tools: a remote MCP client cannot assume the server shares its
filesystem. Portable shares are the corresponding transport contract.

## Every REST route compared with native MCP

The table lists route paths. GET/POST variants, static assets, SSE and protocol
adapters can share paths; an absent tool is not automatically a missing graph
operation. Native MCP dispatch uses the same guarded REST router. The older
Homelab proxy is a separate compatibility surface and is not this inventory.

| REST path | Native MCP tool(s) |
| --- | --- |
| `/` | No dedicated native tool |
| `/.well-known/void` | No dedicated native tool |
| `/acquire` | `quipu_acquire` |
| `/acquire/composed` | `quipu_acquire_composed` |
| `/align/apply` | `quipu_align_apply` |
| `/align/decide` | `quipu_align_decide` |
| `/align/propose` | `quipu_align_propose` |
| `/ask` | `quipu_ask` |
| `/changes` | `quipu_changes` |
| `/changes/stream` | No dedicated native tool |
| `/context` | `quipu_context` |
| `/cooccurrence` | `quipu_cooccurrence` |
| `/cord` | `quipu_cord` |
| `/datalinks.js` | No dedicated native tool |
| `/datasets` | `quipu_datasets` |
| `/embed_backfill` | `quipu_embed_backfill` |
| `/embed_backfill_graph` | `quipu_embed_backfill_graph` |
| `/entity` | `quipu_entity` |
| `/entity/{iri}` | No dedicated native tool |
| `/entity/{iri}/html` | No dedicated native tool |
| `/entity/{iri}/json` | No dedicated native tool |
| `/entity/{iri}/ttl` | No dedicated native tool |
| `/entity_history` | `quipu_entity_history` |
| `/episode` | `quipu_episode` |
| `/episode/retract` | `quipu_retract_episode` |
| `/episodes/complete` | `quipu_episodes_complete` |
| `/events` | `quipu_events` |
| `/events/commit` | `quipu_events_commit` |
| `/events/stream` | No dedicated native tool |
| `/explain` | `quipu_explain` |
| `/export` | `quipu_export` |
| `/fragments` | `quipu_fragments` |
| `/graph` | `quipu_graph` |
| `/graph-canvas.js` | No dedicated native tool |
| `/graph/create` | `quipu_graph_create` |
| `/graph/freeze` | `quipu_graph_freeze` |
| `/graph/label` | `quipu_graph_label` |
| `/graph/thaw` | `quipu_graph_thaw` |
| `/graphs` | `quipu_graph_list` |
| `/health` | `quipu_health` |
| `/hybrid_search` | `quipu_hybrid_search` |
| `/impact` | `quipu_impact` |
| `/import` | `quipu_import` |
| `/import/promote` | `quipu_import_promote` |
| `/knot` | `quipu_knot` |
| `/knot/promote` | `quipu_knot_promote` |
| `/knot/stage` | `quipu_knot_stage` |
| `/merge/apply` | `quipu_merge_apply` |
| `/merge/decisions` | `quipu_merge_decisions` |
| `/metrics` | No dedicated native tool |
| `/ontology` | `quipu_load_ontology` |
| `/overlay/compose` | `quipu_overlay_compose` |
| `/overlay/create` | `quipu_overlay_create` |
| `/overlay/write` | `quipu_overlay_write` |
| `/path/backtest` | `quipu_path_backtest` |
| `/path/cone` | `quipu_path_cone` |
| `/policy/check` | `quipu_policy_check` |
| `/preview/{iri}` | No dedicated native tool |
| `/project` | `quipu_project` |
| `/proposal/accept` | `quipu_accept_proposal` |
| `/proposal/reject` | `quipu_reject_proposal` |
| `/proposals` | `quipu_list_proposals` |
| `/propose` | `quipu_propose_schema_change` |
| `/queries` | `quipu_queries` |
| `/query` | `quipu_query` |
| `/quipu-components.js` | No dedicated native tool |
| `/rdf-graph-store` | No dedicated native tool |
| `/reason` | `quipu_reason` |
| `/reconcile` | `quipu_reconcile` |
| `/report` | `quipu_report` |
| `/resolve` | `quipu_resolve_entity` |
| `/retract` | `quipu_retract` |
| `/retract/source` | `quipu_retract_source` |
| `/search` | `quipu_search` |
| `/search/nodes` | No dedicated native tool |
| `/search_facts` | `quipu_search_facts` |
| `/search_nodes` | `quipu_search_nodes` |
| `/search_query` | `quipu_search_query` |
| `/set` | `quipu_set` |
| `/shapes` | `quipu_shapes` |
| `/share` | `quipu_share` |
| `/spotlight` | `quipu_spotlight` |
| `/stats` | `quipu_stats` |
| `/subscriptions` | `quipu_subscriptions` |
| `/transactions` | `quipu_transactions` |
| `/ui` | No dedicated native tool |
| `/unified_search` | `quipu_unified_search` |
| `/unravel` | `quipu_unravel` |
| `/update` | `quipu_update` |
| `/validate` | `quipu_validate` |
| `/vendor/three.module.min.js` | No dedicated native tool |
| `/verdict/verify` | `quipu_verdict_verify` |
| `/verifier/authorized` | `quipu_verifier_authorized` |
| `/version` | `quipu_version` |
| `/workflow/capabilities` | `quipu_workflow_capabilities` |

## Every top-level CLI verb compared with native MCP

Every top-level dispatch verb is listed below. File operations, installation,
Git hooks and configuration diagnostics retain local authority; a remote server
is not assumed to share the client filesystem. A similarly named tool is not
asserted to have the same options or behavior.

| CLI verb | Native MCP counterpart or gap |
| --- | --- |
| `align` | quipu_align_propose / quipu_align_decide / quipu_align_apply |
| `attest` | Local identity registration; no native tool |
| `audit` | Local verification audit; no native tool |
| `changes` | quipu_changes |
| `compose` | Local composition/share workflow; no equivalent native tool |
| `cord` | quipu_cord |
| `db` | Local database/file maintenance |
| `demotions` | Local premise demotions; no native tool |
| `diff-textconv` | Local Git integration |
| `doctor` | Local label doctor; no native tool |
| `episode` | quipu_episode |
| `events` | quipu_events / quipu_events_commit |
| `explain` | quipu_explain (owl) |
| `export` | quipu_export |
| `fork` | Local transaction fork lifecycle |
| `gate` | Local quiescent-copy shadow gate; no native tool |
| `git-merge` | Local Git integration |
| `graph` | See the graph-operation table |
| `help` | Client help, not a tool |
| `hook` | Local session hook |
| `hooks` | Local session hook |
| `impact` | quipu_impact |
| `import` | quipu_import / quipu_import_promote |
| `ingest` | Local directory ingestion |
| `install-alias` | Local executable installation |
| `knot` | quipu_knot |
| `load` | quipu_knot (RDF); file/bundle loading is local |
| `mcp` | The protocol server, not a tool |
| `merge` | Local share merge; quipu_merge_decisions / quipu_merge_apply are the portable decision workflow |
| `merge-driver` | Local Git integration |
| `migrate-vectors` | Local vector migration; no native tool |
| `ontology` | quipu_load_ontology (owl) |
| `pack` | Local pack/file production |
| `path` | quipu_path_cone / quipu_path_backtest |
| `pendant-check` | Local Git integration |
| `pendant-resolve` | Local Git integration |
| `policy` | Local policy drafting/backtesting; quipu_policy_check is a different operation |
| `project` | quipu_project |
| `propose` | quipu_propose_schema_change / quipu_list_proposals / quipu_accept_proposal / quipu_reject_proposal |
| `query` | quipu_query |
| `read` | quipu_query |
| `reason` | quipu_reason |
| `repl` | Interactive client, not a tool |
| `report` | quipu_report |
| `restore` | Local database/file replacement |
| `retract` | quipu_retract |
| `search` | quipu_search (configuration/options differ) |
| `search-index` | Local index lifecycle; no native tool |
| `shapes` | quipu_shapes |
| `share` | quipu_share |
| `stats` | quipu_stats (server store; CLI local --db selection stays local) |
| `status` | Local project-bundle status |
| `trust-root` | Local root trust bootstrap; no native tool |
| `unpack` | Local pack merge; portable import uses quipu_import / quipu_import_promote |
| `unravel` | quipu_unravel |
| `validate` | quipu_validate |

## Coverage and deliberate transport differences

Portable share/import/promotion, Datalog reasoning, SPARQL Update, subscription
administration and consumer cursors now have explicit native MCP tools. The
same guarded REST router owns each operation; no separate store mutation path
or raw arbitrary-route proxy is introduced. Entity/history, bounded acquisition,
embedding backfill, search-query fusion, reconciliation, fragments and snapshot
staging/promotion are also advertised. `quipu_workflow_capabilities` lets a
client check workflow enforcement before using the update surface.

- Entity format suffixes are content-negotiation alternatives to `quipu_entity`
  JSON-LD, not additional graph mutations. History retains the existing endpoint's
  MAIN-only semantics; this work does not repair composed audit completeness.
- Static UI assets, HTML preview cards, RDF service discovery and metrics
  retain HTTP presentation/protocol contracts. Server health, version and stats
  also have MCP read tools; CLI --db selects a local store rather than the server.
  No remote service-management authority is added.
- SSE routes retain their streaming HTTP protocol. The same durable events and
  changes are accessible via bounded MCP pull tools, with explicit cursor commit.
- RDF Graph Store is an HTTP-standard alternative; MCP scoped knot/query/update
  use existing graph validation. No claim of identical wire encoding is made.
- Local packs, file attachment/import/restore, Git integration, installation,
  index migration, doctor/audit, trust-root bootstrap and hooks retain local
  filesystem/process authority. A remote tool cannot safely assume the caller's
  path exists on the server. Portable share/import is the remote transfer path.
- REST aliases such as `/search/nodes` and entity format suffixes reuse existing
  search/entity semantics; protocol and output differences remain explicit.

Every advertised native tool maps to a registered classified REST route; the
transport test checks that direction, including routes split across modules.
The top-level CLI table covers portable counterparts and explicit local-only
operations. Subcommand option identity and composed-history correctness are not
implied by manifest coverage. The book's fresh-store recipe and workflow
acceptance probes verify behavior separately from this inventory.
