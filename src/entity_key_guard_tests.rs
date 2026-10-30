use super::*;

fn allocated() -> (Store, String) {
    let mut store = store_with(&[ENDPOINT], &shapes("", "1"));
    let iri = mint(&mut store, &json!({HOST:"db.example",PORT:5432})).unwrap()["iri"]
        .as_str()
        .unwrap()
        .to_string();
    (store, iri)
}

#[test]
fn assertions_and_type_removal_cannot_evade_the_key_guard() {
    let (mut store, iri) = allocated();
    let entity = store.lookup(&iri).unwrap().unwrap();
    let port = store.intern(PORT).unwrap();
    let typed = store.intern(crate::namespace::RDF_TYPE).unwrap();
    let ty = store.intern(ENDPOINT).unwrap();
    for (attribute, value, op) in [
        (port, crate::Value::Int(5433), crate::Op::Assert),
        (port, crate::Value::Int(5432), crate::Op::Retract),
        (typed, crate::Value::Ref(ty), crate::Op::Retract),
    ] {
        let before = store.latest_tx_id().unwrap();
        assert!(
            store
                .transact(
                    &[crate::store::Datum {
                        entity,
                        attribute,
                        value,
                        op,
                        valid_from: crate::time::now_iso(),
                        valid_to: None
                    }],
                    &crate::time::now_iso(),
                    None,
                    None
                )
                .is_err()
        );
        assert_eq!(store.latest_tx_id().unwrap(), before);
    }
}

#[test]
fn shape_freeze_compares_effective_contract_and_retains_history() {
    let (store, iri) = allocated();
    // Default hash and explicit hash are the same derivation. Unrelated
    // constraints may evolve; a lexical/order/kind change cannot.
    store
        .load_shapes(
            "endpoint",
            &shapes("quipu:keyStrategy \"hash\" ;", "1"),
            &crate::time::now_iso(),
        )
        .unwrap();
    store
        .load_shapes("other", "# unrelated shape set", &crate::time::now_iso())
        .unwrap();
    store.remove_shapes("other").unwrap();
    for candidate in [
        shapes("", "2"),
        shapes("", "1").replace(
            &format!("( <{HOST}> <{PORT}> )"),
            &format!("( <{PORT}> <{HOST}> )"),
        ),
        shapes_kinds(
            "",
            "1",
            &format!("sh:datatype <{XSD_STRING}>"),
            &format!("sh:datatype <{XSD_STRING}>"),
        ),
    ] {
        assert!(
            store
                .load_shapes("endpoint", &candidate, &crate::time::now_iso())
                .is_err()
        );
    }
    let entity = store.lookup(&iri).unwrap().unwrap();
    store
        .conn
        .execute(
            "UPDATE facts SET valid_to=?1 WHERE e=?2",
            rusqlite::params![crate::time::now_iso(), entity],
        )
        .unwrap();
    assert!(
        store.remove_shapes("endpoint").is_err(),
        "retirement cannot unfreeze identity"
    );
    let empty = store_with(&[ENDPOINT], &shapes("", "1"));
    empty
        .load_shapes(
            "endpoint",
            &shapes("quipu:keyStrategy \"lexical\" ;", "1"),
            &crate::time::now_iso(),
        )
        .unwrap();
}

#[test]
fn optional_verify_recomputes_exact_local_reference_without_writing() {
    let (mut store, iri) = allocated();
    let tx = store.latest_tx_id().unwrap();
    let result = tool_entity_ref(&mut store, &json!({"action":"verify","iri":iri})).unwrap();
    assert_eq!(result["verified"], true);
    assert_eq!(store.latest_tx_id().unwrap(), tx);
    assert!(
        tool_entity_ref(
            &mut store,
            &json!({"action":"verify","iri":"https://public.example/ep/unknown"})
        )
        .is_err()
    );
    assert!(tool_entity_ref(&mut store, &json!({"action":"verify","iri":iri,"keys":{}})).is_err());
    let entity = store.lookup(&iri).unwrap().unwrap();
    let port = store.lookup(PORT).unwrap().unwrap();
    // A received/legacy corrupt graph is detectable without silently repairing it.
    store
        .conn
        .execute(
            "UPDATE facts SET v=?1 WHERE e=?2 AND a=?3",
            rusqlite::params![crate::Value::Int(5433).to_bytes(), entity, port],
        )
        .unwrap();
    assert!(tool_entity_ref(&mut store, &json!({"action":"verify","iri":iri})).is_err());
    // Matching lexical text with the wrong RDF kind must also refuse.
    store
        .conn
        .execute(
            "UPDATE facts SET v=?1 WHERE e=?2 AND a=?3",
            rusqlite::params![crate::Value::Str("5432".into()).to_bytes(), entity, port],
        )
        .unwrap();
    assert!(tool_entity_ref(&mut store, &json!({"action":"verify","iri":iri})).is_err());
}

#[test]
fn two_forks_with_the_same_key_converge_and_divergent_port_has_one_decision() {
    for divergent in [false, true] {
        let mut root = store_with(&[ENDPOINT], &shapes("", "1"));
        let base = root.latest_tx_id().unwrap();
        let a = root
            .fork_create("writer-a", base, &crate::time::now_iso(), None)
            .unwrap();
        let b = root
            .fork_create("writer-b", base, &crate::time::now_iso(), None)
            .unwrap();
        let mut writer_a = store_with(&[ENDPOINT], &shapes("", "1"));
        let mut writer_b = store_with(&[ENDPOINT], &shapes("", "1"));
        let first = mint(&mut writer_a, &json!({HOST:"db.example",PORT:5432})).unwrap();
        let second = mint(&mut writer_b, &json!({HOST:"db.example",PORT:5432})).unwrap();
        assert_eq!(first["iri"], second["iri"]);
        let iri = first["iri"].as_str().unwrap();
        for (graph, port) in [(a.g, 5432), (b.g, if divergent { 5433 } else { 5432 })] {
            // A forced bad incoming fork tests the operator, not permission to
            // mutate an existing key: these graphs have no prior identity yet.
            let text = format!(
                "<{iri}> <{}> <{ENDPOINT}> .\n<{iri}> <{HOST}> \"db.example\" .\n<{iri}> <{PORT}> \"{port}\"^^<{XSD_INTEGER}> .",
                crate::namespace::RDF_TYPE
            );
            crate::rdf::ingest_rdf_to_graph(
                &mut root,
                text.as_bytes(),
                oxrdfio::RdfFormat::NTriples,
                None,
                &crate::time::now_iso(),
                None,
                None,
                graph,
            )
            .unwrap();
        }
        assert!(matches!(
            root.fork_promote("writer-a", &crate::time::now_iso(), None)
                .unwrap(),
            crate::store::forks::ForkPromotion::Promoted { .. }
        ));
        let before = root.latest_tx_id().unwrap();
        match root
            .fork_promote("writer-b", &crate::time::now_iso(), None)
            .unwrap()
        {
            crate::store::forks::ForkPromotion::Conflicts(decisions) if divergent => {
                assert_eq!(decisions.len(), 1);
                assert_eq!(root.latest_tx_id().unwrap(), before);
            }
            crate::store::forks::ForkPromotion::Promoted { asserted, .. } if !divergent => {
                assert_eq!(asserted, 0);
            }
            other => panic!("unexpected merge: {other:?}"),
        }
        let entity = root.lookup(iri).unwrap().unwrap();
        assert_eq!(root.entity_facts(entity).unwrap().len(), 3);
    }
}

#[test]
fn rdf_type_itself_may_be_a_key_field_and_cannot_be_replaced() {
    let type_predicate = crate::namespace::RDF_TYPE;
    let ttl = format!(
        "@prefix sh: <http://www.w3.org/ns/shacl#> . @prefix q: <https://quipu.dev/ontology/> . <urn:example:shape> a sh:NodeShape ; sh:targetClass <{ENDPOINT}> ; q:keyFields (<{type_predicate}>) ; sh:property [sh:path <{type_predicate}> ; sh:minCount 1 ; sh:maxCount 1 ; sh:nodeKind sh:IRI] ."
    );
    let mut store = store_with(&[ENDPOINT], &ttl);
    let iri = mint(&mut store, &json!({type_predicate:{"@id":ENDPOINT}})).unwrap()["iri"]
        .as_str()
        .unwrap()
        .to_string();
    let entity = store.lookup(&iri).unwrap().unwrap();
    let attribute = store.intern(type_predicate).unwrap();
    let other = store.intern("urn:example:Other").unwrap();
    assert!(
        store
            .transact(
                &[crate::store::Datum {
                    entity,
                    attribute,
                    value: crate::Value::Ref(other),
                    op: crate::Op::Assert,
                    valid_from: crate::time::now_iso(),
                    valid_to: None
                }],
                &crate::time::now_iso(),
                None,
                None
            )
            .is_err()
    );
}

#[test]
fn named_graph_keys_are_guarded_and_verification_does_not_fall_back_to_root() {
    let mut store = store_with(&[ENDPOINT], &shapes("", "1"));
    let graph_iri = "urn:example:received";
    let graph = store.graph_create(graph_iri).unwrap();
    let result = tool_entity_ref(
        &mut store,
        &json!({"action":"mint","namespace":"ep","type":ENDPOINT,
            "graph":graph_iri,"keys":{HOST:"db.example",PORT:5432}}),
    )
    .unwrap();
    let iri = result["iri"].as_str().unwrap();
    let before = store.latest_tx_id().unwrap();
    assert!(tool_entity_ref(&mut store, &json!({"action":"verify","iri":iri})).is_err());
    assert_eq!(
        tool_entity_ref(
            &mut store,
            &json!({"action":"verify","iri":iri,"graph":graph_iri})
        )
        .unwrap()["verified"],
        true
    );
    assert!(
        tool_entity_ref(
            &mut store,
            &json!({"action":"verify","iri":iri,"graph":"urn:example:unknown"})
        )
        .is_err()
    );
    assert!(store.lookup("urn:example:unknown").unwrap().is_none());
    assert_eq!(store.latest_tx_id().unwrap(), before);
    let entity = store.lookup(iri).unwrap().unwrap();
    let attribute = store.lookup(PORT).unwrap().unwrap();
    assert!(
        store
            .transact_to_graph(
                &[crate::store::Datum {
                    entity,
                    attribute,
                    value: crate::Value::Int(5433),
                    op: crate::Op::Assert,
                    valid_from: crate::time::now_iso(),
                    valid_to: None,
                }],
                &crate::time::now_iso(),
                None,
                None,
                graph,
            )
            .is_err()
    );
    assert_eq!(store.latest_tx_id().unwrap(), before);
}
