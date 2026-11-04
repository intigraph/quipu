use super::*;

fn route(name: &str) -> Route {
    Route {
        graph: format!("https://example.test/graphs/{name}"),
        file_id: name.to_owned(),
        store_id: format!("store-{name}"),
        log_epoch: format!("epoch-{name}"),
        generation: 4,
        policy: "policy-v1".into(),
    }
}

fn budget() -> Budget {
    Budget {
        max_owners: 2,
        queued_per_owner: 3,
        readers_per_owner: 1,
        max_queued: 6,
        max_connections: 6,
    }
}

fn catalog(routes: Vec<Route>) -> Result<Catalog> {
    Catalog::new(7, "legacy", routes, budget())
}

fn request(graph: Option<&str>, generation: Option<u64>) -> WriteRequest<'_> {
    WriteRequest {
        graph,
        catalog_revision: 7,
        owner_generation: generation,
        policy: "policy-v1",
        signed: false,
        cross_graph_context: false,
    }
}

#[test]
fn exact_graph_identity_selects_one_owner_without_changing_legacy() {
    let a = route("a");
    let c = catalog(vec![a.clone(), route("b")]).unwrap();
    assert_eq!(
        c.write_target(&request(Some(&a.graph), Some(4))).unwrap(),
        WriteTarget::Routed(&a)
    );
    for graph in [None, Some("https://example.test/graphs/unknown")] {
        assert_eq!(
            c.write_target(&request(graph, None)).unwrap(),
            WriteTarget::Legacy
        );
    }
    // No URI decoding, case folding or prefix matching in routing decisions.
    let changed = a.graph.replace("/a", "/%61");
    assert!(c.write_target(&request(Some(&changed), Some(4))).is_err());
    let padded = format!(" {} ", a.graph);
    assert!(c.write_target(&request(Some(&padded), None)).is_err());
    assert_eq!(c.revision(), 7);
    assert_eq!(c.routes().count(), 2);
}

#[test]
fn duplicate_graph_file_store_and_legacy_lineage_are_refused() {
    let a = route("a");
    for duplicate in ["graph", "file", "store", "legacy"] {
        let mut b = route("b");
        match duplicate {
            "graph" => {
                b.graph.clone_from(&a.graph);
                b.generation += 1;
            }
            "file" => b.file_id.clone_from(&a.file_id),
            "store" => b.store_id.clone_from(&a.store_id),
            "legacy" => b.store_id = "legacy".into(),
            _ => unreachable!(),
        }
        assert!(catalog(vec![a.clone(), b]).is_err(), "{duplicate}");
    }
    assert!(catalog(vec![a, route("b")]).is_ok());
}

#[test]
fn paths_and_unpinned_or_invalid_routes_are_not_admitted() {
    for value in ["../a", "a/b", "/tmp/a", "a.db", "", ".", "a%2fb"] {
        let mut a = route("a");
        a.file_id = value.into();
        assert!(catalog(vec![a]).is_err(), "{value}");
    }
    let mut a = route("a");
    a.file_id = "a".repeat(65);
    assert!(catalog(vec![a]).is_err());
    for graph in [
        "",
        "relative",
        "urn:quipu:graph:root",
        " https://example.test/a",
    ] {
        let mut a = route("a");
        a.graph = graph.into();
        assert!(catalog(vec![a]).is_err(), "{graph}");
    }
    for pin in ["generation", "store", "epoch", "policy"] {
        let mut a = route("a");
        match pin {
            "generation" => a.generation = 0,
            "store" => a.store_id.clear(),
            "epoch" => a.log_epoch.clear(),
            "policy" => a.policy.clear(),
            _ => unreachable!(),
        }
        assert!(catalog(vec![a]).is_err(), "{pin}");
    }
}

#[test]
fn aggregate_reservations_cannot_overflow_or_borrow_another_owner_budget() {
    let routes = vec![route("a"), route("b")];
    assert!(catalog(routes.clone()).is_ok());
    for kind in ["owners", "queue", "connections", "overflow", "zero"] {
        let mut b = budget();
        match kind {
            "owners" => b.max_owners = 1,
            "queue" => b.max_queued = 5,
            "connections" => b.max_connections = 5,
            "overflow" => {
                b.queued_per_owner = usize::MAX;
                b.max_queued = usize::MAX;
            }
            "zero" => b.readers_per_owner = 0,
            _ => unreachable!(),
        }
        assert!(
            Catalog::new(7, "legacy", routes.clone(), b).is_err(),
            "{kind}"
        );
    }
    let mut b = budget();
    b.readers_per_owner = usize::MAX;
    b.max_connections = usize::MAX;
    assert!(Catalog::new(7, "legacy", routes, b).is_err());
}

#[test]
fn stale_pins_signed_replay_and_external_context_refuse_without_fallback() {
    let a = route("a");
    let c = catalog(vec![a.clone()]).unwrap();
    for kind in [
        "catalog",
        "generation",
        "missing",
        "policy",
        "signed",
        "context",
    ] {
        let mut r = request(Some(&a.graph), Some(4));
        match kind {
            "catalog" => r.catalog_revision = 6,
            "generation" => r.owner_generation = Some(3),
            "missing" => r.owner_generation = None,
            "policy" => r.policy = "policy-v0",
            "signed" => r.signed = true,
            "context" => r.cross_graph_context = true,
            _ => unreachable!(),
        }
        assert!(c.write_target(&r).is_err(), "{kind}");
    }
    let mut legacy = request(None, None);
    legacy.signed = true;
    legacy.cross_graph_context = true;
    assert_eq!(c.write_target(&legacy).unwrap(), WriteTarget::Legacy);
}

#[test]
fn mixed_file_batch_is_refused_even_when_the_first_selection_is_valid() {
    let (a, b) = (route("a"), route("b"));
    let c = catalog(vec![a.clone(), b.clone()]).unwrap();
    let a_req = || request(Some(&a.graph), Some(4));
    assert_eq!(
        c.write_batch(&[a_req(), a_req()]).unwrap(),
        WriteTarget::Routed(&a)
    );
    for mixed in [
        vec![a_req(), request(Some(&b.graph), Some(4))],
        vec![a_req(), request(None, None)],
        vec![request(None, None), a_req()],
    ] {
        assert!(c.write_batch(&mixed).is_err());
    }
    assert!(c.write_batch(&[]).is_err());
    assert_eq!(
        c.write_batch(&[request(None, None), request(Some("urn:legacy:g"), None)])
            .unwrap(),
        WriteTarget::Legacy
    );
}
