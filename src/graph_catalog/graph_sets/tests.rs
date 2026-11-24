use super::*;

fn set(name: &str, members: &[&str]) -> GraphSet {
    GraphSet {
        owner: Route {
            graph: format!("urn:{name}"),
            file_id: name.into(),
            store_id: format!("store-{name}"),
            log_epoch: format!("epoch-{name}"),
            generation: 3,
            policy: "policy-v1".into(),
        },
        graphs: members.iter().map(|s| (*s).into()).collect(),
    }
}

fn budget() -> Budget {
    Budget {
        max_owners: 2,
        queued_per_owner: 1,
        readers_per_owner: 1,
        max_queued: 2,
        max_connections: 6,
    }
}

fn catalog() -> GraphSetCatalog {
    GraphSetCatalog::new(
        7,
        "legacy",
        vec![
            set(
                "application",
                &["urn:application", "urn:ephemeral", "urn:provenance"],
            ),
            set("other", &["urn:other"]),
        ],
        budget(),
        4,
    )
    .unwrap()
}

fn request(graph: &str) -> WriteRequest<'_> {
    WriteRequest {
        graph: Some(graph),
        catalog_revision: 7,
        owner_generation: Some(3),
        policy: "policy-v1",
        signed: false,
        cross_graph_context: false,
    }
}

#[test]
fn application_data_and_provenance_plan_one_atomic_file_without_changing_old_catalog() {
    let c = catalog();
    let plan = c
        .plan_batch(&[
            request("urn:provenance"),
            request("urn:application"),
            request("urn:ephemeral"),
        ])
        .unwrap();
    assert_eq!(plan.owner.file_id, "application");
    assert_eq!(
        plan.graphs,
        ["urn:application", "urn:ephemeral", "urn:provenance"]
    );
    assert_eq!(c.physical().routes().count(), 2);
    // Four logical graphs still reserve only two writers/reader pools.
    // 2 * (writer + fence + reader) + legacy + scratch = 8 connections.
    assert!(c.physical().memory_allocation(512, 1, 1).is_ok());
    assert!(c.physical().memory_allocation(511, 1, 1).is_err());
    // The old API does not accidentally advertise aliases as writable routes.
    assert!(
        c.physical()
            .write_target(&request("urn:provenance"))
            .is_err()
    );
}

#[test]
fn composed_selection_groups_snapshots_and_does_not_expand_membership() {
    let c = catalog();
    let plan = c
        .plan_read(&["urn:other", "urn:provenance", "urn:application"])
        .unwrap();
    assert_eq!(plan.len(), 2);
    assert_eq!(plan[0].owner.graph, "urn:application");
    assert_eq!(plan[0].graphs, ["urn:application", "urn:provenance"]);
    assert_eq!(plan[1].graphs, ["urn:other"]);
    assert!(c.plan_read(&["urn:application", "urn:unknown"]).is_err());
    assert!(
        c.plan_read(&["urn:application", "urn:application"])
            .is_err()
    );
    assert!(c.plan_read(&[crate::schema::ROOT_GRAPH_IRI]).is_err());
    assert!(c.plan_read(&[]).is_err());
}

#[test]
fn batch_admission_checks_every_member_before_any_writer_is_selected() {
    let c = catalog();
    for bad in [
        "urn:other",
        "urn:missing",
        " urn:application",
        crate::schema::ROOT_GRAPH_IRI,
    ] {
        assert!(
            c.plan_batch(&[request("urn:application"), request(bad)])
                .is_err(),
            "{bad}"
        );
    }
    assert!(
        c.plan_batch(&[request("urn:application"), request("urn:application")])
            .is_err()
    );
    for pin in [
        "catalog",
        "generation",
        "policy",
        "signed",
        "context",
        "implicit",
    ] {
        let mut r = request("urn:provenance");
        match pin {
            "catalog" => r.catalog_revision += 1,
            "generation" => r.owner_generation = None,
            "policy" => r.policy = "other",
            "signed" => r.signed = true,
            "context" => r.cross_graph_context = true,
            "implicit" => r.graph = None,
            _ => unreachable!(),
        }
        assert!(
            c.plan_batch(&[request("urn:application"), r]).is_err(),
            "{pin}"
        );
    }
}

#[test]
fn duplicate_physical_owners_and_logical_members_are_never_inferred_as_one() {
    for collision in ["file", "lineage", "member", "owner", "epoch"] {
        let a = set("application", &["urn:application", "urn:provenance"]);
        let mut b = set("other", &["urn:other"]);
        match collision {
            "file" => b.owner.file_id.clone_from(&a.owner.file_id),
            "lineage" => b.owner.store_id.clone_from(&a.owner.store_id),
            "member" => b.graphs.push("urn:provenance".into()),
            "owner" => b.owner.graph = "urn:application".into(),
            "epoch" => {
                b.owner.file_id.clone_from(&a.owner.file_id);
                b.owner.log_epoch = "changed".into();
            }
            _ => unreachable!(),
        }
        assert!(
            GraphSetCatalog::new(7, "legacy", vec![a, b], budget(), 5).is_err(),
            "{collision}"
        );
    }
}

#[test]
fn graph_membership_and_physical_reservations_have_independent_bounds() {
    for members in [
        vec![],
        vec!["urn:other"],
        vec!["urn:application", "relative"],
        vec!["urn:application", crate::schema::ROOT_GRAPH_IRI],
        vec!["urn:application", "urn:application"],
    ] {
        assert!(
            GraphSetCatalog::new(7, "legacy", vec![set("application", &members)], budget(), 4)
                .is_err()
        );
    }
    assert!(
        GraphSetCatalog::new(
            7,
            "legacy",
            vec![set("application", &["urn:application", "urn:provenance"])],
            budget(),
            1
        )
        .is_err()
    );
    let mut b = budget();
    b.max_owners = 1;
    assert!(
        GraphSetCatalog::new(
            7,
            "legacy",
            vec![
                set("application", &["urn:application"]),
                set("other", &["urn:other"])
            ],
            b,
            4
        )
        .is_err()
    );
}
