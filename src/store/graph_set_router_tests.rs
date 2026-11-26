use super::*;
use crate::{
    graph_catalog::{Budget, GraphSet, Route},
    store::Datum,
    types::{Op, Value as TermValue},
};
use serde_json::{Value, json};
use std::path::PathBuf;

const P: &str = "https://example.test/project/p";
const PROV: &str = "https://example.test/project/p#provenance";
const EPH: &str = "https://example.test/project/p#ephemeral";
const MEMBERS: [&str; 3] = [P, PROV, EPH];

struct Fixture {
    _dir: tempfile::TempDir,
    root: PathBuf,
    legacy: Store,
    route: Route,
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let path = root.join("tracker.db");
    {
        let store = Store::open(path.to_str().unwrap()).unwrap();
        for graph in MEMBERS {
            store.graph_create(graph).unwrap();
        }
    }
    Store::adopt(path.to_str().unwrap()).unwrap();
    let store = Store::open(path.to_str().unwrap()).unwrap();
    let route = Route {
        graph: P.into(),
        file_id: "tracker".into(),
        store_id: store.store_id().unwrap(),
        log_epoch: store.log_epoch().unwrap(),
        generation: 4,
        policy: "policy-v1".into(),
    };
    drop(store);
    let legacy = Store::open(root.join("legacy.db").to_str().unwrap()).unwrap();
    // The legacy file retains its pre-cutover copies of the members, so only
    // the fence can refuse a write that bypasses the router.
    for graph in MEMBERS {
        legacy.graph_create(graph).unwrap();
    }
    Fixture {
        _dir: dir,
        root,
        legacy,
        route,
    }
}

fn catalog_for(route: &Route) -> Arc<GraphSetCatalog> {
    Arc::new(
        GraphSetCatalog::new(
            7,
            "legacy-store",
            vec![GraphSet {
                owner: route.clone(),
                graphs: MEMBERS.iter().map(|g| (*g).into()).collect(),
            }],
            Budget {
                max_owners: 1,
                queued_per_owner: 1,
                readers_per_owner: 1,
                max_queued: 1,
                max_connections: 3,
            },
            3,
        )
        .unwrap(),
    )
}

fn owner(f: &Fixture, catalog: &Arc<GraphSetCatalog>) -> GraphSetOwner {
    let store = Store::open(f.root.join("tracker.db").to_str().unwrap()).unwrap();
    let allocation = catalog.physical().memory_allocation(320, 1, 1).unwrap();
    GraphSetOwner::prepare(catalog.clone(), &f.root, P, store, "policy-v1", allocation).unwrap()
}

fn fence_all(legacy: &Store, generation: u64) {
    for graph in MEMBERS {
        legacy.fence_routed_graph(graph, 7, generation).unwrap();
    }
}

fn request(graph: Option<&str>) -> WriteRequest<'_> {
    WriteRequest {
        graph,
        catalog_revision: 7,
        owner_generation: graph.map(|_| 4),
        policy: "policy-v1",
        signed: false,
        cross_graph_context: false,
    }
}

fn all_members() -> Vec<WriteRequest<'static>> {
    MEMBERS.iter().map(|g| request(Some(g))).collect()
}

fn batch(router: &GraphSetRouter, route: &Route) -> GraphSetBatch {
    let datum = {
        let store = router.owner(P).unwrap().owner.writer.lock();
        Datum {
            entity: store.intern("urn:item").unwrap(),
            attribute: store.intern("urn:value").unwrap(),
            value: TermValue::Str("present".into()),
            valid_from: "2026-01-01T00:00:00Z".into(),
            valid_to: None,
            op: Op::Assert,
        }
    };
    GraphSetBatch {
        store_id: route.store_id.clone(),
        log_epoch: route.log_epoch.clone(),
        graphs: MEMBERS
            .iter()
            .map(|g| ((*g).to_string(), vec![datum.clone()]))
            .collect(),
        timestamp: "2026-01-01T00:00:00Z".into(),
        actor: Some("router-test".into()),
        source: Some("router-test".into()),
    }
}

fn facts(store: &Store) -> i64 {
    store
        .conn
        .query_row("SELECT count(*) FROM facts", [], |r| r.get(0))
        .unwrap()
}

fn owner_facts(router: &GraphSetRouter) -> i64 {
    router
        .owner(P)
        .unwrap()
        .owner
        .read(|s| Ok(facts(s)))
        .unwrap()
}

fn denied(result: Result<GraphSetRouter>, why: &str) {
    match result {
        Err(Error::PolicyDenied(msg)) => {
            assert!(msg.contains("graph-set router"), "{why}: {msg}");
        }
        Err(other) => panic!("{why}: unexpected {other}"),
        Ok(_) => panic!("{why}: router admitted"),
    }
}

fn knot(graph: Option<&str>, tag: &str) -> Value {
    let mut input = json!({
        "turtle": format!("<urn:set-router:{tag}> <urn:set-router:p> \"v\" ."),
        "source": "set-router-test",
    });
    if let Some(graph) = graph {
        input["graph"] = json!(graph);
    }
    input
}

#[test]
fn admission_requires_the_fence_to_name_every_member_exactly() {
    let f = fixture();
    let catalog = catalog_for(&f.route);
    denied(
        GraphSetRouter::admit(catalog.clone(), &f.legacy, vec![owner(&f, &catalog)]),
        "unfenced legacy",
    );
    // Fencing only the owner graph would leave provenance and ephemeral
    // writable in the legacy file: refused.
    f.legacy.fence_routed_graph(P, 7, 4).unwrap();
    denied(
        GraphSetRouter::admit(catalog.clone(), &f.legacy, vec![owner(&f, &catalog)]),
        "owner graph fenced, other members not",
    );

    let g = fixture();
    let catalog = catalog_for(&g.route);
    fence_all(&g.legacy, 3);
    denied(
        GraphSetRouter::admit(catalog.clone(), &g.legacy, vec![owner(&g, &catalog)]),
        "fenced at a stale generation",
    );

    let h = fixture();
    let catalog = catalog_for(&h.route);
    fence_all(&h.legacy, 4);
    h.legacy
        .fence_routed_graph("https://example.test/elsewhere", 7, 4)
        .unwrap();
    denied(
        GraphSetRouter::admit(catalog.clone(), &h.legacy, vec![owner(&h, &catalog)]),
        "fence names a graph the catalog does not route",
    );
}

#[test]
fn every_physical_owner_needs_exactly_one_owner_from_this_catalog() {
    let f = fixture();
    let catalog = catalog_for(&f.route);
    fence_all(&f.legacy, 4);
    denied(
        GraphSetRouter::admit(catalog.clone(), &f.legacy, Vec::new()),
        "route without an owner",
    );
    let other = catalog_for(&f.route);
    let foreign = owner(&f, &other);
    denied(
        GraphSetRouter::admit(catalog.clone(), &f.legacy, vec![foreign]),
        "identical route from another catalog",
    );
    GraphSetRouter::admit(catalog.clone(), &f.legacy, vec![owner(&f, &catalog)]).expect("admitted");
}

#[test]
fn batches_go_to_their_set_owner_or_are_left_to_legacy_never_both() {
    let mut f = fixture();
    let catalog = catalog_for(&f.route);
    fence_all(&f.legacy, 4);
    let router =
        GraphSetRouter::admit(catalog.clone(), &f.legacy, vec![owner(&f, &catalog)]).unwrap();
    let batch = batch(&router, &f.route);

    // Routed: every member commits in the one owner; legacy is untouched.
    let legacy_before = facts(&f.legacy);
    assert_eq!(router.target(&all_members()).unwrap(), SetTarget::Routed(P));
    let heads = router.transact(&all_members(), &batch).unwrap();
    assert_eq!(heads.len(), 3);
    assert_eq!(owner_facts(&router), 3);
    assert_eq!(
        facts(&f.legacy),
        legacy_before,
        "routed batch leaked to legacy"
    );

    // Legacy: classified, never executed or forwarded by the router.
    let root = [
        request(None),
        request(Some("https://example.test/reference")),
    ];
    assert_eq!(router.target(&root).unwrap(), SetTarget::Legacy);
    let err = router.transact(&root, &batch).unwrap_err().to_string();
    assert!(err.contains("ordinary legacy writer"), "{err}");

    // Default ROOT parity: the ordinary legacy pipeline still writes ROOT
    // after fencing and admission, and nothing reaches the owner.
    crate::tool_knot(&mut f.legacy, &knot(None, "root")).expect("legacy ROOT write");
    assert!(facts(&f.legacy) > legacy_before, "ROOT landed in legacy");
    assert_eq!(owner_facts(&router), 3, "ROOT leaked to the owner");

    // Mixed legacy + routed refuses before any writer.
    let mixed = [request(None), request(Some(PROV))];
    assert!(router.target(&mixed).is_err());
    assert!(router.transact(&mixed, &batch).is_err());

    // A stale owner generation refuses without partial data.
    let mut stale = all_members();
    stale[2].owner_generation = Some(3);
    assert!(router.transact(&stale, &batch).is_err());
    assert_eq!(owner_facts(&router), 3);
}

#[test]
fn bypassing_the_router_cannot_write_any_member_into_the_legacy_file() {
    let mut f = fixture();
    let catalog = catalog_for(&f.route);
    fence_all(&f.legacy, 4);
    let _router =
        GraphSetRouter::admit(catalog.clone(), &f.legacy, vec![owner(&f, &catalog)]).unwrap();
    let before = facts(&f.legacy);
    for (i, graph) in MEMBERS.iter().enumerate() {
        let err = crate::tool_knot(&mut f.legacy, &knot(Some(graph), &format!("bypass-{i}")))
            .unwrap_err()
            .to_string();
        assert!(err.contains("routed to its own owner"), "{graph}: {err}");
    }
    assert_eq!(facts(&f.legacy), before);
    // Positive twin: an unrouted named graph is still writable in legacy.
    f.legacy
        .graph_create("https://example.test/reference")
        .unwrap();
    crate::tool_knot(
        &mut f.legacy,
        &knot(Some("https://example.test/reference"), "reference"),
    )
    .expect("unrouted legacy graph write");
    assert!(facts(&f.legacy) > before);
}

#[test]
fn recheck_notices_a_removed_member_trigger() {
    let f = fixture();
    let catalog = catalog_for(&f.route);
    fence_all(&f.legacy, 4);
    let router =
        GraphSetRouter::admit(catalog.clone(), &f.legacy, vec![owner(&f, &catalog)]).unwrap();
    router.recheck(&f.legacy).unwrap();
    f.legacy
        .conn
        .execute_batch("DROP TRIGGER quipu_routed_fence_facts_g_insert")
        .unwrap();
    assert!(router.recheck(&f.legacy).is_err());
}
