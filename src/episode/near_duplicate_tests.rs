//! Write admission, not merely a resolver hint: no production graph probes.
use super::*;

fn episode(name: &str, node: &str, kind: &str) -> Episode {
    serde_json::from_value(serde_json::json!({
        "name": name, "nodes": [{"name": node, "type": kind}], "edges": []
    }))
    .unwrap()
}

#[test]
fn rt9f_today_pairs_require_adjudication_before_commit() {
    let mut admitted = Vec::new();
    for (existing, incoming, kind) in [
        ("controller", "controller.lan", "LXCContainer"),
        ("CT217", "ct-217", "LXCContainer"),
        ("ct-236", "CT-236-guest", "LXCContainer"),
        ("Quipu", "quipu", "WebApplication"),
    ] {
        let mut store = crate::Store::open_in_memory().unwrap();
        let ns = crate::namespace::DEFAULT_BASE_NS;
        ingest_episode(
            &mut store,
            &episode("seed", existing, kind),
            "2026-11-07",
            ns,
        )
        .unwrap();
        let before = store.latest_tx_id().unwrap();
        let opts = IngestResolutionOpts {
            enabled: true,
            threshold: 0.85,
            top_k: 3,
            strict_mode: true,
        };
        let result = ingest_episode_with_resolution(
            &mut store,
            &episode("candidate", incoming, kind),
            "2026-11-07",
            ns,
            Some(&opts),
        );
        if result.is_ok() {
            admitted.push((existing, incoming));
        } else {
            assert_eq!(
                before,
                store.latest_tx_id().unwrap(),
                "refusal must precede commit"
            );
        }
    }
    assert!(
        admitted.is_empty(),
        "unadjudicated duplicates admitted: {admitted:?}"
    );
}

#[test]
fn rt9f_distinct_ids_kinds_and_existing_iri_remain_writable() {
    for (existing, incoming, kind, incoming_kind) in [
        ("ct-236", "ct-243", "LXCContainer", "LXCContainer"),
        ("CT217", "CT218", "LXCContainer", "LXCContainer"),
        (
            "ct-236-guest",
            "ct-237-guest",
            "LXCContainer",
            "LXCContainer",
        ),
        ("hypervisor", "hypervisor", "BareMetalHost", "BareMetalHost"),
        (
            "controller",
            "controller.lan",
            "TerraformResource",
            "LXCContainer",
        ),
        ("ticket-abcd", "ticket-abce", "WorkItem", "WorkItem"),
    ] {
        let mut store = crate::Store::open_in_memory().unwrap();
        let ns = crate::namespace::DEFAULT_BASE_NS;
        ingest_episode(
            &mut store,
            &episode("seed", existing, kind),
            "2026-11-07",
            ns,
        )
        .unwrap();
        let opts = IngestResolutionOpts {
            enabled: true,
            threshold: 0.85,
            top_k: 1,
            strict_mode: true,
        };
        ingest_episode_with_resolution(
            &mut store,
            &episode("new", incoming, incoming_kind),
            "2026-11-07",
            ns,
            Some(&opts),
        )
        .unwrap();
    }
}

#[test]
fn rt9f_reasoned_distinction_is_durable_and_empty_reason_refuses() {
    let ns = crate::namespace::DEFAULT_BASE_NS;
    let mut store = crate::Store::open_in_memory().unwrap();
    ingest_episode(
        &mut store,
        &episode("seed", "controller", "LXCContainer"),
        "2026-11-07",
        ns,
    )
    .unwrap();
    let opts = IngestResolutionOpts {
        enabled: true,
        threshold: 0.85,
        top_k: 3,
        strict_mode: true,
    };
    let mut candidate = episode("distinct", "controller.lan", "LXCContainer");
    candidate.nodes[0]
        .distinct_from
        .push(format!("{ns}controller"));
    let before = store.latest_tx_id().unwrap();
    assert!(
        ingest_episode_with_resolution(&mut store, &candidate, "2026-11-07", ns, Some(&opts))
            .is_err()
    );
    assert_eq!(before, store.latest_tx_id().unwrap());
    candidate.nodes[0].properties = Some(
        serde_json::json!({
            "distinctReason": "Separate test guests in independent identity domains"
        })
        .as_object()
        .unwrap()
        .clone(),
    );
    ingest_episode_with_resolution(&mut store, &candidate, "2026-11-07", ns, Some(&opts)).unwrap();
    let target = node_iri("controller.lan", ns);
    assert!(
        crate::resolution::recorded_distinct_from(&store, &target)
            .unwrap()
            .contains(&format!("{ns}controller"))
    );
}

#[test]
fn rt9f_aliases_in_one_batch_are_not_invisible() {
    let mut store = crate::Store::open_in_memory().unwrap();
    let ns = crate::namespace::DEFAULT_BASE_NS;
    let mut ep = episode("batch", "CT217", "LXCContainer");
    ep.nodes
        .push(episode("unused", "ct-217", "LXCContainer").nodes.remove(0));
    let opts = IngestResolutionOpts {
        enabled: true,
        threshold: 0.85,
        top_k: 3,
        strict_mode: true,
    };
    assert!(
        ingest_episode_with_resolution(&mut store, &ep, "2026-11-07", ns, Some(&opts)).is_err()
    );
    assert_eq!(store.latest_tx_id().unwrap(), 0);
}

#[test]
fn rt9f_numeric_aliases_ignore_fuzzy_threshold() {
    let ns = crate::namespace::DEFAULT_BASE_NS;
    for (left, right) in [("CT101", "CT-101-guest"), ("ct-1", "ct-0000001-guest")] {
        for (first, second) in [(left, right), (right, left)] {
            for kind in [
                "LXCContainer",
                "quechua:LXCContainer",
                "https://w3id.org/intigraph/camayoc/core/ns#LXCContainer",
            ] {
                for threshold in [0.85, 0.99] {
                    for batch in [false, true] {
                        let mut store = crate::Store::open_in_memory().unwrap();
                        let mut ep = episode("candidate", second, kind);
                        if batch {
                            ep.nodes
                                .push(episode("unused", first, kind).nodes.remove(0));
                        } else {
                            ingest_episode(
                                &mut store,
                                &episode("seed", first, kind),
                                "2026-11-08",
                                ns,
                            )
                            .unwrap();
                        }
                        let before = store.latest_tx_id().unwrap();
                        let opts = IngestResolutionOpts {
                            enabled: true,
                            threshold,
                            top_k: 1,
                            strict_mode: true,
                        };
                        assert!(
                            ingest_episode_with_resolution(
                                &mut store,
                                &ep,
                                "2026-11-08",
                                ns,
                                Some(&opts)
                            )
                            .is_err(),
                            "missed {first}/{second} {kind} threshold={threshold} batch={batch}"
                        );
                        assert_eq!(store.latest_tx_id().unwrap(), before);
                    }
                }
            }
        }
    }
}

#[test]
fn rt9f_prefixed_distinct_ids_and_work_items_remain_writable() {
    let ns = crate::namespace::DEFAULT_BASE_NS;
    for (left, right, kind) in [
        ("ct-236", "ct-243", "quechua:LXCContainer"),
        ("ticket-abcd", "ticket-abce", "quechua:WorkItem"),
        ("ticket-abcd", "ticket-abce", "quechua:Bead"),
    ] {
        let mut store = crate::Store::open_in_memory().unwrap();
        ingest_episode(&mut store, &episode("seed", left, kind), "2026-11-08", ns).unwrap();
        let opts = IngestResolutionOpts {
            enabled: true,
            threshold: 0.85,
            top_k: 1,
            strict_mode: true,
        };
        ingest_episode_with_resolution(
            &mut store,
            &episode("candidate", right, kind),
            "2026-11-08",
            ns,
            Some(&opts),
        )
        .unwrap();
    }
}

#[test]
fn rt9f_hint_mode_is_explicitly_advisory() {
    let mut store = crate::Store::open_in_memory().unwrap();
    let ns = crate::namespace::DEFAULT_BASE_NS;
    ingest_episode(
        &mut store,
        &episode("seed", "controller", "LXCContainer"),
        "2026-11-07",
        ns,
    )
    .unwrap();
    let opts = IngestResolutionOpts {
        enabled: true,
        threshold: 0.85,
        top_k: 3,
        strict_mode: false,
    };
    let result = ingest_episode_with_resolution(
        &mut store,
        &episode("hint", "controller.lan", "LXCContainer"),
        "2026-11-07",
        ns,
        Some(&opts),
    )
    .unwrap();
    assert!(!result.resolution_hints.is_empty());
}

#[test]
fn rt9f_legacy_distinction_is_symmetric_and_honored() {
    for predicate in [
        "https://quipu.dev/ontology/distinctFrom",
        "http://example.org/domain/distinctFrom",
    ] {
        for reverse in [false, true] {
            let mut store = crate::Store::open_in_memory().unwrap();
            store.set_base_ns("http://example.org/domain/");
            let left = "http://example.org/controller";
            let right = "http://example.org/controller.lan";
            let (subject, object) = if reverse {
                (right, left)
            } else {
                (left, right)
            };
            let turtle = format!("<{subject}> <{predicate}> <{object}> .");
            crate::rdf::ingest_rdf(
                &mut store,
                turtle.as_bytes(),
                oxrdfio::RdfFormat::Turtle,
                None,
                "2026-11-07",
                None,
                None,
            )
            .unwrap();
            assert!(
                crate::resolution::recorded_distinct_from(&store, left)
                    .unwrap()
                    .contains(&right.to_string()),
                "missing durable distinction: {predicate}, reverse={reverse}"
            );
        }
    }
}
