use super::*;
use crate::graph_catalog::{Budget, Route};
use serde_json::json;
use std::path::PathBuf;

const GRAPH: &str = "https://example.test/graphs/ingest";

struct Fixture {
    _dir: tempfile::TempDir,
    root: PathBuf,
    legacy: Store,
    route: Route,
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let owner_path = root.join("ingest.db");
    let owner_path = owner_path.to_str().unwrap();
    Store::open(owner_path)
        .unwrap()
        .graph_create(GRAPH)
        .unwrap();
    Store::adopt(owner_path).unwrap();
    let owner = Store::open(owner_path).unwrap();
    let route = Route {
        graph: GRAPH.into(),
        file_id: "ingest".into(),
        store_id: owner.store_id().unwrap(),
        log_epoch: owner.log_epoch().unwrap(),
        generation: 4,
        policy: "policy-v1".into(),
    };
    drop(owner);
    let legacy = Store::open(root.join("legacy.db").to_str().unwrap()).unwrap();
    Fixture {
        _dir: dir,
        root,
        legacy,
        route,
    }
}

fn catalog_for(route: &Route) -> Arc<Catalog> {
    Arc::new(
        Catalog::new(
            7,
            "legacy-store",
            vec![route.clone()],
            Budget {
                max_owners: 1,
                queued_per_owner: 1,
                readers_per_owner: 1,
                max_queued: 1,
                max_connections: 4,
            },
        )
        .unwrap(),
    )
}

fn owner(f: &Fixture, catalog: &Arc<Catalog>) -> Owner {
    let store = Store::open(f.root.join("ingest.db").to_str().unwrap()).unwrap();
    Owner::prepare(catalog.clone(), &f.root, GRAPH, store, "policy-v1").unwrap()
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

fn input(graph: Option<&str>, tag: &str) -> Value {
    let mut input = json!({
        "turtle": format!("<urn:router:{tag}> <urn:router:p> \"v\" ."),
        "source": "router-test",
    });
    if let Some(graph) = graph {
        input["graph"] = json!(graph);
    }
    input
}

fn facts(store: &Store) -> i64 {
    store
        .conn
        .query_row("SELECT count(*) FROM facts", [], |r| r.get(0))
        .unwrap()
}

fn owner_facts(router: &Router) -> i64 {
    router.owner(GRAPH).unwrap().read(|s| Ok(facts(s))).unwrap()
}

fn denied(result: Result<Router>, why: &str) {
    match result {
        Err(Error::PolicyDenied(msg)) => assert!(msg.contains("graph router"), "{why}: {msg}"),
        Err(other) => panic!("{why}: unexpected {other}"),
        Ok(_) => panic!("{why}: router admitted"),
    }
}

#[test]
fn no_route_is_advertised_until_the_legacy_file_fences_it_exactly() {
    let f = fixture();
    let catalog = catalog_for(&f.route);
    denied(
        Router::admit(catalog.clone(), &f.legacy, vec![owner(&f, &catalog)]),
        "unfenced legacy",
    );
    f.legacy.fence_routed_graph(GRAPH, 7, 3).unwrap();
    denied(
        Router::admit(catalog.clone(), &f.legacy, vec![owner(&f, &catalog)]),
        "fenced at a stale generation",
    );

    let g = fixture();
    let catalog = catalog_for(&g.route);
    g.legacy.fence_routed_graph(GRAPH, 7, 4).unwrap();
    g.legacy
        .fence_routed_graph("https://example.test/graphs/elsewhere", 7, 4)
        .unwrap();
    denied(
        Router::admit(catalog.clone(), &g.legacy, vec![owner(&g, &catalog)]),
        "fence names a graph the catalog does not route",
    );
}

#[test]
fn every_route_needs_exactly_one_owner() {
    let f = fixture();
    let catalog = catalog_for(&f.route);
    f.legacy.fence_routed_graph(GRAPH, 7, 4).unwrap();
    denied(
        Router::admit(catalog.clone(), &f.legacy, Vec::new()),
        "route without an owner",
    );
    Router::admit(catalog.clone(), &f.legacy, vec![owner(&f, &catalog)]).expect("admitted");
}

#[test]
fn writes_go_to_their_owner_or_legacy_and_never_both() {
    let mut f = fixture();
    let catalog = catalog_for(&f.route);
    // The legacy file still registers the graph (its retained pre-cutover
    // copy), so only the fence can refuse a bypassing write below.
    f.legacy.graph_create(GRAPH).unwrap();
    f.legacy.fence_routed_graph(GRAPH, 7, 4).unwrap();
    let router = Router::admit(catalog.clone(), &f.legacy, vec![owner(&f, &catalog)]).unwrap();
    let legacy_before = facts(&f.legacy);
    let owner_before = owner_facts(&router);

    router
        .knot(&mut f.legacy, &request(None), &input(None, "root"))
        .expect("legacy ROOT write");
    let legacy_after_root = facts(&f.legacy);
    assert!(legacy_after_root > legacy_before, "ROOT landed in legacy");
    assert_eq!(
        owner_facts(&router),
        owner_before,
        "ROOT leaked to the owner"
    );

    router
        .knot(
            &mut f.legacy,
            &request(Some(GRAPH)),
            &input(Some(GRAPH), "routed"),
        )
        .expect("routed write");
    assert!(
        owner_facts(&router) > owner_before,
        "routed write landed in owner"
    );
    assert_eq!(
        facts(&f.legacy),
        legacy_after_root,
        "routed write leaked to legacy"
    );

    // Request and input must name the same graph.
    assert!(
        router
            .knot(
                &mut f.legacy,
                &request(None),
                &input(Some(GRAPH), "mismatch")
            )
            .is_err()
    );
    // Bypassing the router does not reach the legacy file either.
    let err = crate::tool_knot(&mut f.legacy, &input(Some(GRAPH), "bypass"))
        .unwrap_err()
        .to_string();
    assert!(err.contains("routed to its own owner"), "{err}");
    assert_eq!(facts(&f.legacy), legacy_after_root);
}

#[test]
fn recheck_notices_a_removed_trigger() {
    let f = fixture();
    let catalog = catalog_for(&f.route);
    f.legacy.fence_routed_graph(GRAPH, 7, 4).unwrap();
    let router = Router::admit(catalog.clone(), &f.legacy, vec![owner(&f, &catalog)]).unwrap();
    router.recheck(&f.legacy).unwrap();
    f.legacy
        .conn
        .execute_batch("DROP TRIGGER quipu_routed_fence_facts_g_insert")
        .unwrap();
    assert!(router.recheck(&f.legacy).is_err());
}
