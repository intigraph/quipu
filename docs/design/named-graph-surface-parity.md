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
| Import local database file | `graph import file --as IRI` | Portable share `/import` then `/import/promote` | No native portable-share tool yet |

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
| `/acquire` | No dedicated native tool |
| `/acquire/composed` | No dedicated native tool |
| `/align/apply` | `quipu_align_apply` |
| `/align/decide` | `quipu_align_decide` |
| `/align/propose` | `quipu_align_propose` |
| `/ask` | `quipu_ask` |
| `/changes` | No dedicated native tool |
| `/changes/stream` | No dedicated native tool |
| `/context` | `quipu_context` |
| `/cooccurrence` | `quipu_cooccurrence` |
| `/cord` | `quipu_cord` |
| `/datalinks.js` | No dedicated native tool |
| `/datasets` | `quipu_datasets` |
| `/done` | No dedicated native tool |
| `/embed_backfill` | No dedicated native tool |
| `/embed_backfill_graph` | No dedicated native tool |
| `/entity` | No dedicated native tool |
| `/entity/{iri}` | No dedicated native tool |
| `/entity/{iri}/html` | No dedicated native tool |
| `/entity/{iri}/json` | No dedicated native tool |
| `/entity/{iri}/ttl` | No dedicated native tool |
| `/entity_history` | No dedicated native tool |
| `/episode` | `quipu_episode` |
| `/episode/retract` | `quipu_retract_episode` |
| `/episodes/complete` | `quipu_episodes_complete` |
| `/events` | No dedicated native tool |
| `/events/commit` | No dedicated native tool |
| `/events/stream` | No dedicated native tool |
| `/explain` | `quipu_explain` |
| `/export` | `quipu_export` |
| `/fragments` | No dedicated native tool |
| `/graph` | `quipu_graph` |
| `/graph-canvas.js` | No dedicated native tool |
| `/graph/create` | `quipu_graph_create` |
| `/graph/freeze` | `quipu_graph_freeze` |
| `/graph/label` | `quipu_graph_label` |
| `/graph/thaw` | `quipu_graph_thaw` |
| `/graphs` | `quipu_graph_list` |
| `/health` | No dedicated native tool |
| `/hybrid_search` | `quipu_hybrid_search` |
| `/impact` | `quipu_impact` |
| `/import` | No dedicated native tool |
| `/import/promote` | No dedicated native tool |
| `/knot` | `quipu_knot` |
| `/knot/promote` | No dedicated native tool |
| `/knot/stage` | No dedicated native tool |
| `/merge/apply` | `quipu_merge_apply` |
| `/merge/decisions` | `quipu_merge_decisions` |
| `/metrics` | No dedicated native tool |
| `/ontology` | `quipu_load_ontology` |
| `/overlay/compose` | `quipu_overlay_compose` |
| `/overlay/create` | `quipu_overlay_create` |
| `/overlay/write` | `quipu_overlay_write` |
| `/path/backtest` | `quipu_path_backtest` |
| `/path/cone` | `quipu_path_cone` |
| `/pending` | No dedicated native tool |
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
| `/reason` | No dedicated native tool |
| `/reconcile` | No dedicated native tool |
| `/report` | `quipu_report` |
| `/resolve` | `quipu_resolve_entity` |
| `/retract` | `quipu_retract` |
| `/retract/source` | `quipu_retract_source` |
| `/search` | `quipu_search` |
| `/search/nodes` | No dedicated native tool |
| `/search_facts` | `quipu_search_facts` |
| `/search_nodes` | `quipu_search_nodes` |
| `/search_query` | No dedicated native tool |
| `/set` | `quipu_set` |
| `/shapes` | `quipu_shapes` |
| `/share` | No dedicated native tool |
| `/spotlight` | No dedicated native tool |
| `/stats` | No dedicated native tool |
| `/subscriptions` | No dedicated native tool |
| `/transactions` | No dedicated native tool |
| `/ui` | No dedicated native tool |
| `/unified_search` | `quipu_unified_search` |
| `/unravel` | `quipu_unravel` |
| `/update` | No dedicated native tool |
| `/validate` | `quipu_validate` |
| `/vendor/three.module.min.js` | No dedicated native tool |
| `/verdict/verify` | `quipu_verdict_verify` |
| `/verifier/authorized` | `quipu_verifier_authorized` |
| `/version` | No dedicated native tool |
| `/workflow/capabilities` | No dedicated native tool |

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
| `changes` | No native changes tool |
| `compose` | Local composition/share workflow; no equivalent native tool |
| `cord` | quipu_cord |
| `db` | Local database/file maintenance |
| `demotions` | Local premise demotions; no native tool |
| `diff-textconv` | Local Git integration |
| `doctor` | Local label doctor; no native tool |
| `episode` | quipu_episode |
| `events` | No native event reader/cursor tool |
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
| `import` | No native portable-import tool |
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
| `reason` | No native reasoning tool |
| `repl` | Interactive client, not a tool |
| `report` | quipu_report |
| `restore` | Local database/file replacement |
| `retract` | quipu_retract |
| `search` | quipu_search (configuration/options differ) |
| `search-index` | Local index lifecycle; no native tool |
| `shapes` | quipu_shapes |
| `share` | No native portable-share tool |
| `stats` | No native stats tool |
| `status` | Local project-bundle status |
| `trust-root` | Local root trust bootstrap; no native tool |
| `unpack` | Local pack merge; portable import is a native-MCP gap |
| `unravel` | quipu_unravel |
| `validate` | quipu_validate |

## Remaining gaps and deliberate transport differences

- Native MCP lacks portable share/import/promote, although CLI/REST and the
  Homelab compatibility proxy support them. This is a functional coverage gap.
- Native MCP lacks `reason`, subscriptions/cursor administration, and a SPARQL
  Update tool. REST exposes these; the CLI has `reason` and event readers. These
  are functional coverage gaps, not empty graph results.
- Search-query fusion, entity/history, embedding backfill and acquisition
  workflows also have REST-only dedicated entry points. Query/cord/search cover
  some reads, but do not certify equivalent options or workflow actions.
- Static UI resources, health/version/metrics, RDF Graph Store protocol and SSE
  streams are transport-specific. Their lack of identically named MCP tools is
  intentional; ordinary graph reads/writes remain available through MCP tools.
- CLI pack files, local database attachment/import/restore, git integration,
  installation and hooks require local filesystem/process authority. They are
  not silently represented as remote file-path operations.

This inventory does **not** claim full global CLI/REST/MCP parity. The named
creation/label recipe is complete; the functional gaps above must be resolved
or explicitly dispositioned before closing a task that requires global parity.
