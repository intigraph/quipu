//! Quipu REST API server — HTTP interface to the knowledge graph.
//! Usage: `quipu-server [--db <path>] [--bind <addr>]`

use std::sync::Arc;

use axum::{
    Router,
    routing::{get, post},
};

// A bin crate root resolves `mod x;` to `src/x.rs`, which would collide with
// the library's modules — so each submodule names its path under `src/server/`
// explicitly.
#[path = "server/admission.rs"]
mod admission;
#[path = "server/align.rs"]
mod align;
#[path = "server/assets.rs"]
mod assets;
#[path = "server/attest.rs"]
mod attest;
#[path = "server/auth.rs"]
mod auth;
#[path = "server/auth_guard.rs"]
mod auth_guard;
#[path = "server/auth_response.rs"]
mod auth_response;
#[path = "server/background.rs"]
mod background;
#[path = "server/base.rs"]
mod base;
#[path = "server/cors.rs"]
mod cors;
#[path = "server/deck.rs"]
mod deck;
#[path = "server/entity.rs"]
mod entity;
#[path = "server/feed.rs"]
mod feed;
#[path = "server/graph_backfill.rs"]
mod graph_backfill;
#[path = "server/graph_metrics.rs"]
mod graph_metrics;
#[path = "server/graph_store.rs"]
mod graph_store;
#[path = "server/graph_store_compact.rs"]
mod graph_store_compact;
#[path = "server/handle.rs"]
mod handle;
#[path = "server/input_fields.rs"]
mod input_fields;
#[path = "server/memory_files.rs"]
mod memory_files;
#[cfg(test)]
#[path = "server/namespace_tests.rs"]
mod namespace_tests;
#[path = "server/namespaces.rs"]
mod namespaces;
#[cfg(feature = "owl")]
#[path = "server/owl_materialize.rs"]
mod owl_materialize;
#[cfg(test)]
#[path = "server/parse_guard_tests.rs"]
mod parse_guard_tests;
#[path = "server/patch.rs"]
mod patch;
#[path = "server/point_reads.rs"]
mod point_reads;
#[path = "server/process_pool.rs"]
mod process_pool;
#[path = "server/projections.rs"]
mod projections;
#[path = "server/publication.rs"]
mod publication;
#[path = "server/query_capture.rs"]
mod query_capture;
mod query_endpoint;
#[path = "server/query_usage.rs"]
mod query_usage;
#[path = "server/reason.rs"]
mod reason;
#[path = "server/relay.rs"]
mod relay;
#[path = "server/replica.rs"]
mod replica;
#[path = "server/request_middleware.rs"]
mod request_middleware;
#[path = "server/scoped_reads.rs"]
mod scoped_reads;
#[path = "server/service_description.rs"]
mod service_description;
#[path = "server/snapshot_upload.rs"]
mod snapshot_upload;
#[cfg(test)]
#[path = "server/tests.rs"]
mod tests;
#[path = "server/tools.rs"]
mod tools;
#[path = "server/update.rs"]
mod update;
#[path = "server/update_eval.rs"]
mod update_eval;
#[path = "server/update_slice.rs"]
mod update_slice;
#[path = "server/wal_maintenance.rs"]
mod wal_maintenance;

use base::metrics_handler;
use entity::{
    entity_conneg, entity_history, entity_html, entity_json, entity_query_conneg,
    entity_turtle_suffix, fragments_handler, preview_handler, reconcile_handler, spotlight_handler,
};
use feed::{changes_get, events_commit, events_get, transactions};
#[path = "server/event_cursor.rs"]
mod event_cursor;
#[path = "server/feed_stream.rs"]
mod feed_stream;
pub(crate) use handle::{ReadPool, SharedStore, StoreHandle};
use namespaces::namespaces_get;
#[cfg(test)]
use query_endpoint::query;
use reason::{explain, reason, shapes};
use tools::*;

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    // Asking the binary who it is must NOT touch disk (aegis-j0nq). These are
    // pure reads of compiled-in constants and must stay above Store::open.
    if base::handle_identity_args(&args) {
        return;
    }

    let config = base::load_config(&args);
    scoped_reads::validate(&config, &args);
    let memory_allocation = process_pool::startup_allocation(&config, &args);

    let db_path = config.store_path.to_string_lossy().to_string();
    let bind_addr = config.server.bind.clone();

    // warn LOUDLY about documented, settable knobs this server does not
    // act on (vector.backend = lancedb, federation.remotes), rather than accepting
    // them and silently doing nothing.
    for warning in config.unwired_warnings() {
        eprintln!("warning: {warning}");
    }

    let mut store = replica::open_store(&db_path, &config).unwrap_or_else(|e| {
        eprintln!("error opening store {db_path}: {e}");
        std::process::exit(1);
    });
    if let Some(allocation) = memory_allocation {
        store
            .apply_memory_allocation(allocation)
            .unwrap_or_else(|error| {
                eprintln!("cannot apply memory allocation: {error}");
                std::process::exit(1)
            });
    }

    // quipu-lv7: `vector.backend` selects the store's vector backend in-binary.
    // It refuses when this build cannot construct the configured one rather
    // than falling back to the SQLite table a migrated deployment has left.
    let is_replica = config.server.replica.is_some();
    // A relay writes only through its applier (s24.2 S4): like a replica it
    // runs no writer jobs, observers or derivations, which would put local
    // transactions with no source in the log it serves from.
    let is_follower = is_replica || config.server.relay.is_some();
    if !is_replica {
        base::apply_vector_backend(&mut store, &config);
    }

    // quipu-at2: announce what the `[[quipu.attachments]]` declarations became.
    // A composed layer that nobody can see is how "it returned no rows" becomes
    // a mystery; a missing file already refused the open above.
    base::report_attachments(&store);

    // The configured base namespace and entity-resolution policy; see
    // base::apply_identity_policies for why both must precede the first write.
    base::apply_identity_policies(&mut store, &config);

    // Apply search/limit guardrails so callers can't request unbounded result
    // sets or scan the whole fact log (hq-gkd).
    if is_replica {
        replica::apply_search(&mut store, &config.search).unwrap_or_else(|e| {
            eprintln!("error configuring replica search: {e}");
            std::process::exit(1);
        });
    } else {
        base::apply_search(&mut store, &config.search);
    }
    // quipu #68: the floors and their consumer land together — a settable knob
    // that nothing reads is the bug config.rs guards against.
    store.labels_config_mut().clone_from(&config.label_floors);

    // quipu #47: report the configured federation remotes at startup, and prove
    // they are REACHED rather than merely parsed. `federation.remotes` was
    // parsed, exported, and consumed by nothing for months; announcing what was
    // configured — and whether each one answers — is what makes the difference
    // visible without waiting for a federated query to be issued.
    if !config.federation.remotes.is_empty() {
        base::report_federation(&store, &config.federation);
    }

    // Apply the SHACL validation policy so episode writes can be gated against
    // persistently-loaded shapes, not just episode-inline shapes (hq-c6s).
    store.shacl_config_mut().clone_from(&config.shacl);
    if config.shacl.validate_on_write {
        eprintln!("SHACL write-validation enabled (loaded shapes enforced on every write)");
    }

    // Apply the OWL write-time constraint policy and opt-in reactive
    // materialization (aegis-bmqup, quipu-923) — see base::apply_owl for why
    // the clone_from is load-bearing.
    if !is_follower {
        base::apply_owl(&mut store, &config);
    }

    // Current-state projections (aegis-tg5 F2) — see projections::install_at_startup.
    projections::install_at_startup(&mut store, &config, is_follower);

    // Register the reactive reasoner so DERIVED facts stay fresh on write
    // (aegis-nnf0h). It was registered at src/cli.rs:582 and NOWHERE else, so the
    // server — the only writer that matters, since every agent ingests through it
    // — ran with no incremental derivation at all.
    //
    // Rules come from the SHAPES table: `shapes/aegis-rules.ttl` documents that
    // rules may be stored alongside SHACL shapes, and the parser only picks up
    // `a rule:Rule` subjects, so the two vocabularies coexist and no new storage
    // or route is needed. A deployment with no rules loaded is the normal case and
    // stays silent.
    //
    // SCOPE, stated because the bead's original framing over-promised: this makes
    // DATALOG derivation incremental. `ReactiveReasoner::new` takes a `RuleSet` and
    // there is no OWL path in it, so OWL materialization is re-run on ontology
    // load only. Entailments that must stay live are therefore better
    // expressed as rules — an `owl:inverseOf` is exactly a one-atom projection,
    // `hosts(?y, ?x) :- runs_on(?x, ?y)`.
    //
    // Registered UNCONDITIONALLY (quipu-923, gap G6), rules or none: the
    // startup ruleset used to be a snapshot, so rules loaded through /shapes
    // needed a restart. Now /shapes hot-swaps the ruleset through the Arc kept
    // on StoreHandle, and an empty reasoner is a per-write no-op.
    #[cfg(feature = "reactive-reasoner")]
    let reactive_reasoner = if is_follower {
        None
    } else {
        let empty = || quipu::reasoner::RuleSet::empty(quipu::namespace::DEFAULT_BASE_NS);
        // A malformed ruleset must not take the server down, but it must not
        // pass unremarked either: the failure mode being avoided is a reasoner
        // that silently does nothing.
        let ruleset = match store.get_combined_shapes() {
            Ok(Some(ttl)) => match quipu::reasoner::parse_rules(&ttl, None) {
                Ok(rs) => rs,
                Err(e) => {
                    eprintln!("reactive reasoner starts EMPTY — rules failed to parse: {e}");
                    empty()
                }
            },
            Ok(None) => empty(),
            Err(e) => {
                eprintln!("reactive reasoner starts EMPTY — could not read shapes: {e}");
                empty()
            }
        };
        let n = ruleset.len();
        let reasoner = Arc::new(quipu::ReactiveReasoner::new(ruleset));
        store.add_observer(reasoner.clone());
        if n > 0 {
            eprintln!(
                "reactive reasoner registered — {n} Datalog rule(s) re-derive on every write"
            );
        } else {
            eprintln!("reactive reasoner registered (0 rules) — /shapes loads take effect live");
        }
        Some(reasoner)
    };

    // Apply the governance enforcement policy: when enabled, `boundary:"action"`
    // policies gate every write (the loom's write-time gate, see
    // docs/design/policy-edit-hooks.md).
    store.governance_config_mut().clone_from(&config.governance);
    if config.governance.enforce_on_write {
        eprintln!(
            "Governance write-enforcement enabled (action-boundary policies gate every write)"
        );
    }

    match quipu::embedding_runtime::configure_provider(&mut store, &config.embedding) {
        Ok(status) => eprintln!("{status}"),
        Err(error) => {
            eprintln!("embedding configuration error: {error}");
            std::process::exit(1);
        }
    }
    // Keep expensive inference outside the global writer lock.
    store.set_defer_auto_embed(true);

    // v1 verdict signing (the loom, Phase 0): load-or-generate the host-file
    // ed25519 key. QUIPU_SIGNING_KEY overrides the default path.
    if !is_replica {
        let signing_key_path = quipu::signing::default_key_path();
        match quipu::signing::SigningIdentity::load(&signing_key_path, "quipu") {
            Ok(id) => {
                eprintln!(
                    "verdict signing enabled (verifier=quipu, key={})",
                    signing_key_path.display()
                );
                eprintln!(
                    "  register this public key to trust its verdicts: {}",
                    id.public_key_hex()
                );
                store.set_signing_identity(Arc::new(id));
            }
            Err(e) => eprintln!("warning: verdict signing disabled -- {e}"),
        }
    }

    // Build the read pool LAST, from the fully-configured writer.
    // Order is load-bearing: `adopt_read_config_from` copies the writer's
    // read-relevant policy, so every setter above must already have run. A pool
    // built earlier would serve reads under default search guardrails and a
    // default base namespace, and answer the same question differently
    // depending on which connection happened to take it.
    //
    // An in-memory store is deliberately NOT pooled: each `:memory:` connection
    // is a separate empty database, so a pool there would serve reads from
    // nothing. Same for a store we cannot open read-only — announce it and run
    // serialised rather than fail to boot, since a slow server is recoverable
    // and a dead one is not.
    if !is_replica {
        wal_maintenance::reset_at_startup(&store);
    }

    if config.server.log_query_phases {
        store.enable_statement_profile();
    }
    let read_pool: ReadPool = point_reads::open_pool(&db_path, &store, &config.server);
    admission::init_request_budget_ms(store.search_config().request_timeout_ms);
    admission::init_episode_budget_ms(store.search_config().episode_request_timeout_ms);

    let relay = relay::open_configured(&store, &config);
    let mut handle = StoreHandle::serving(store, read_pool, &db_path, config.federation.clone());
    handle.relay = relay.clone();
    handle.log_query_admission = config.server.log_query_admission;
    handle.log_query_phases = config.server.log_query_phases;
    handle.signed_route_switches(&config.server);
    handle.query_capture = query_capture::start(config.server.query_capture.as_ref(), &db_path);
    handle.replica = config.server.replica.as_ref().map(|r| {
        Arc::new(replica::ReplicaStatus::expecting(
            r.expected_store_id.clone(),
        ))
    });
    #[cfg(feature = "reactive-reasoner")]
    let handle = handle.with_reasoner(reactive_reasoner);
    let state: SharedStore = handle.into_shared();
    let push_store_outer = state.clone();

    // Access-control policy for write endpoints (hq-azs). Decision logic lives
    // in quipu::http_auth (unit-tested); this only wires it into axum.
    let workers = process_pool::start(&config, &args).unwrap_or_else(|error| {
        eprintln!("cannot start read process pool: {error}");
        std::process::exit(1)
    });
    let auth_policy = auth::bearer_policy(&config.server);
    let session_keys = auth::session_key_policy(&config.server);
    let preflight_keys_required = session_keys.required || config.server.scoped_reads.is_some();
    let scoped_profile = config.server.scoped_reads.clone().map(|profile| {
        (
            profile,
            config
                .server
                .crew_credentials_file
                .clone()
                .expect("validated scoped registry"),
        )
    });
    let scoped_source = state.clone();
    let scoped_enabled = scoped_profile.is_some();
    let read_only = base::read_only(&config, is_replica);
    let replica_config = config.server.replica.clone();
    let is_relay = relay.is_some();

    let cors_origins = config.server.cors_allowed_origins.clone();
    let cors = cors::layer(&cors_origins);

    if args.iter().any(|a| a == "--embed-backfill") {
        eprintln!("Running embedding backfill for all entities...");
        let outcome = tools::backfill_embeddings(&state);
        match outcome {
            Ok(outcome) => eprintln!(
                "Backfill complete: {} entities embedded, {} stale snapshots retried",
                outcome.embedded, outcome.stale_retries
            ),
            // The flag is an EXPLICIT request for a capability. Serving on
            // without it produced a healthy-looking process whose semantic
            // search silently returned nothing (quipu #53) — the operator
            // asked for embeddings, so failing to provide them is fatal, not
            // a line of log noise scrolled past at boot.
            Err(e) => {
                eprintln!("error: --embed-backfill failed: {e}");
                eprintln!(
                    "refusing to start without the capability that was explicitly requested; \
                     drop --embed-backfill to start anyway"
                );
                std::process::exit(1);
            }
        }
    }

    let attest_store = state.clone();
    let app = Router::new()
        .route("/memory/read", post(memory_files::read))
        .route("/memory/export", post(memory_files::export))
        .route("/memory/projection", post(memory_files::projection))
        .route("/memory/commit", post(memory_files::commit))
        .route("/memory/receipt", post(memory_files::receipt))
        .layer(axum::extract::DefaultBodyLimit::max(1024 * 1024))
        .layer(axum::Extension(config.memory_files.clone()))
        .merge(assets::routes())
        // Core API
        .merge(base::status_routes())
        .route(
            "/.well-known/void",
            get(service_description::service_description),
        )
        .route("/metrics", get(metrics_handler))
        .merge(query_endpoint::routes())
        .route("/update", post(update::update_post))
        .route("/patch", post(patch::patch_post))
        .route("/diff", get(patch::diff_get))
        .route("/export", post(publication::export))
        .merge(graph_store::routes())
        .route("/share", post(publication::share_payload))
        .merge(publication::acquisition_routes())
        .merge(align::routes())
        .merge(snapshot_upload::routes())
        .route("/cord", post(cord))
        .route("/render", post(render_prepare))
        .route("/graph", post(graph_view))
        .route("/unravel", post(unravel))
        .route("/validate", post(validate))
        .route("/episode", post(episode))
        .route("/search", post(search))
        // Read-only resolution dry-run: "what would resolution
        // say about this name?" WITHOUT writing. Before this route, the answer
        // was observable only as a side effect of POST /episode — consumers
        // (the graph-extract minting gate) had to reimplement the name matcher
        // client-side and post-then-retract to see embedding matches: undo,
        // not prevention.
        .route("/resolve", post(resolve_probe))
        .route("/hybrid_search", post(hybrid_search))
        .route("/search_query", post(search_query))
        .route("/unified_search", post(unified_search))
        .route("/ask", post(ask))
        .route("/search_nodes", post(search_nodes))
        .route("/search_facts", post(search_facts))
        .route("/search/nodes", post(graphiti_search_nodes))
        .route("/episodes/complete", post(episodes_complete))
        .route("/impact", post(impact_analysis))
        .route("/path/cone", post(path_cone))
        .route("/path/backtest", post(path_backtest))
        .merge(deck::routes())
        .merge(attest::routes())
        .route("/retract", post(retract))
        .route("/set", post(set_predicate))
        .route("/episode/retract", post(retract_episode))
        .route("/retract/source", post(retract_source))
        .route("/shapes", post(shapes))
        .route("/projections", post(projections::projections))
        .route("/reason", post(reason))
        .route("/explain", post(explain))
        .route("/ontology", post(ontology))
        .route("/subscriptions", post(subscriptions))
        .route("/datasets", post(datasets))
        .route("/namespaces", get(namespaces_get).post(namespaces))
        .route("/entity_ref", post(entity_ref))
        .route("/queries", post(queries))
        .route("/propose", post(propose_schema_change))
        .route("/proposals", post(list_proposals))
        .route("/proposal/accept", post(accept_proposal))
        .route("/proposal/reject", post(reject_proposal))
        // camayoc-s0h: the registration and labelling primitives existed in
        // the store with no way in from outside. Routing into a named graph
        // without being able to LABEL it yields separate graphs every query
        // still reads at equal trust, so these ship together.
        .route("/graphs", get(graphs_list))
        .route("/graph/create", post(graph_create))
        .route("/graph/label", post(graph_label))
        .route("/graph/freeze", post(graph_freeze))
        .route("/graph/thaw", post(graph_thaw))
        .route("/overlay/create", post(overlay_create))
        .route("/overlay/write", post(overlay_write))
        .route("/overlay/compose", post(overlay_compose))
        .route("/cooccurrence", post(cooccurrence))
        .route("/policy/check", post(policy_check))
        .route("/verifier/authorized", post(verifier_authorized))
        .route("/verdict/verify", post(verdict_verify))
        .route("/project", post(project_graph))
        .route("/report", get(report_get).post(report))
        .route("/context", post(context))
        .route("/embed_backfill", post(embed_backfill))
        .route(
            "/embed_backfill_graph",
            post(graph_backfill::embed_backfill_graph),
        )
        // Entity + history
        .route("/identity", post(auth::session_identity))
        .route("/entity", get(entity_query_conneg))
        .route("/entity/{iri}", get(entity_conneg))
        .route("/entity/{iri}/json", get(entity_json))
        .route("/entity/{iri}/ttl", get(entity_turtle_suffix))
        .route("/entity/{iri}/html", get(entity_html))
        .route("/entity_history", post(entity_history))
        .route("/transactions", get(transactions))
        // Event log pull API (event-log P1)
        .route("/changes", get(changes_get))
        .route("/watermark", get(replica::watermark))
        .route("/replica/ready", get(replica::ready))
        .route("/events", get(events_get))
        .merge(feed_stream::routes())
        .route("/events/commit", post(events_commit))
        .route("/events/graph", get(feed::events_graph_get))
        .route("/events/graph/commit", post(feed::events_graph_commit))
        // Semantic web APIs (Phase 4)
        .route("/spotlight", post(spotlight_handler))
        .route("/fragments", get(fragments_handler))
        .route("/reconcile", post(reconcile_handler))
        .route("/preview/{iri}", get(preview_handler));
    let app = scoped_reads::guard(
        process_pool::route(app, &workers),
        scoped_profile,
        scoped_source,
    );
    let app = auth_guard::layer(
        app,
        auth_guard::Config {
            auth_policy,
            session_keys,
            attest_store,
            replica_config,
            read_only,
            is_relay,
            scoped_enabled,
        },
    )
    // CORS: allow the Bobbin Knowledge tab (and other browser origins) to
    // call /query, /search, /episode, etc. cross-origin, incl. OPTIONS
    // preflight (GH#5). Built above from the configured allowlist.
    .layer(cors)
    // Strict profiles cannot admit an unsigned CORS preflight before the
    // signature verifier. Use a same-origin client or authenticated broker.
    .layer(axum::middleware::from_fn(move |req, next| {
        auth::strict_preflight(preflight_keys_required, req, next)
    }))
    // Body limit: axum's 2MB default silently caps /knot at small graphs —
    // a code-graph promotion of one real repository is ~9MB of Turtle and
    // was refused 413. 64MB bounds a whole-repo promotion with headroom
    // while still refusing runaway bodies.
    .layer(axum::extract::DefaultBodyLimit::max(64 * 1024 * 1024))
    // Request log, outermost so every request (including CORS preflights
    // and auth rejections) leaves a trace. Until this existed, server.log
    // was startup banners only, and a wedged instance left NOTHING to RCA
    // from — the whole incident was reconstructed from gdb.
    // stderr on purpose: stdout is block-buffered under systemd, stderr is
    // not, and the journal timestamps every line itself.
    .layer(axum::middleware::from_fn(request_middleware::log_request))
    .layer(axum::middleware::from_fn_with_state(
        state.clone(),
        replica::headers,
    ))
    .with_state(state);

    let app = process_pool::protect_worker(app);

    if let Some(replica) = config.server.replica {
        replica::start_metadata_reader(push_store_outer, replica);
    } else if let Some(relay) = relay {
        relay::start(push_store_outer, relay);
    } else {
        background::start(push_store_outer, config.events.retention_days);
    }

    // Everything committed before this line is STARTUP in quipu_write_facts_total
    // (aegis-gwkd76): the work every restart repeats.
    quipu::write_kind::set_serving();
    if scoped_enabled {
        scoped_reads::serve_http(app, &bind_addr).await;
    } else {
        quipu::mcp_transport::serve(app, &args, cors_origins, &bind_addr, &db_path).await;
    }
    drop(workers);
}
