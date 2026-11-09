#[cfg(test)]
mod tests {
    use super::*;
    use crate::transaction_auth::{Identity, with_identity};

    pub(super) fn request() -> Append {
        Append {
            fluent: "urn:fluent:count".into(),
            event: "urn:event:one".into(),
            subject: "urn:subject".into(),
            predicate: "urn:count".into(),
            property_shape: "urn:count-shape".into(),
            as_of: "2026-11-08T03:00:00Z".into(),
            value: Scalar::Integer(3),
        }
    }
    pub(super) fn identity() -> Identity {
        Identity {
            principal: "urn:writer:authenticated".into(),
            credential_id: Some("test-credential".into()),
            auth_class: "named_bearer".into(),
            attestation: Default::default(),
        }
    }
    pub(super) fn loaded() -> Store {
        let mut store = Store::open_in_memory().unwrap();
        let shapes = format!(
            "{}\n<urn:count-shape> <http://www.w3.org/ns/shacl#path> <urn:count>; <http://www.w3.org/ns/shacl#targetNode> <urn:subject>; <{}currentValuePolicy> \"sum\" .",
            include_str!("../../shapes/fluents-v1.ttl"),
            crate::namespace::QUIPU
        );
        store
            .load_shapes(
                "quipu-fluents-v1",
                include_str!("../../shapes/fluents-v1.ttl"),
                "2026-11-08T02:00:00Z",
            )
            .unwrap();
        store
            .load_shapes(
                "test-declaration",
                &shapes.replace(include_str!("../../shapes/fluents-v1.ttl"), ""),
                "2026-11-08T02:00:00Z",
            )
            .unwrap();
        store
    }
    #[test]
    fn native_builder_refuses_missing_identity_and_unloaded_schema_without_rows() {
        let mut store = Store::open_in_memory().unwrap();
        let before = store.latest_tx_id().unwrap();
        assert!(
            store
                .append_fluent_native(&request(), 0)
                .unwrap_err()
                .to_string()
                .contains("authenticated")
        );
        assert_eq!(store.latest_tx_id().unwrap(), before);
        with_identity(Some(identity()), || {
            assert!(
                store
                    .append_fluent_native(&request(), 0)
                    .unwrap_err()
                    .to_string()
                    .contains("not loaded")
            );
        });
        assert_eq!(store.latest_tx_id().unwrap(), before);
        assert!(store.current_facts().unwrap().is_empty());
    }
    #[cfg(not(feature = "shacl"))]
    #[test]
    fn native_builder_cannot_commit_without_native_shacl_support() {
        let mut store = loaded();
        let before = store.latest_tx_id().unwrap();
        let result = with_identity(Some(identity()), || {
            store.append_fluent_native(&request(), 0)
        });
        assert!(
            matches!(result, Err(crate::Error::CannotVerify(_))),
            "{result:?}"
        );
        assert_eq!(store.latest_tx_id().unwrap(), before);
        assert!(store.current_facts().unwrap().is_empty());
    }
    #[cfg(feature = "shacl")]
    #[test]
    fn native_source_order_is_assigned_and_refusals_leave_prior_log_unchanged() {
        let mut store = loaded();
        with_identity(Some(identity()), || {
            let mut request = request();
            let tx = store.append_fluent_native(&request, 0).unwrap();
            assert_eq!(store.transaction_auth(tx).unwrap(), Some(identity()));
            let log = read_log(&store, 0, &request.fluent, Policy::Sum).unwrap();
            let event = log.observations(&request.fluent)[0];
            assert_eq!(event.order.tx, u64::try_from(tx).unwrap());
            assert_eq!(event.order.writer, identity().principal);
            assert_eq!(
                event.recorded_on,
                store.get_transaction(tx).unwrap().unwrap().timestamp
            );
            assert_eq!(store.append_fluent_native(&request, 0).unwrap(), tx);
            assert_eq!(store.latest_tx_id().unwrap(), tx);
            request.value = Scalar::Integer(5);
            assert!(store.append_fluent_native(&request, 0).is_err());
            request.event = "urn:event:two".into();
            request.value = Scalar::Integer(i64::MAX);
            assert!(store.append_fluent_native(&request, 0).is_err());
            assert_eq!(
                read_log(&store, 0, &request.fluent, Policy::Sum).unwrap(),
                log
            );
            request.value = Scalar::Integer(4);
            request.subject = "urn:not-selected".into();
            assert!(store.append_fluent_native(&request, 0).is_err());
            request.subject = "urn:subject".into();
            let next = store.append_fluent_native(&request, 0).unwrap();
            assert!(next > tx);
            let now = read_log(&store, 0, &request.fluent, Policy::Sum)
                .unwrap()
                .current(&request.fluent, Policy::Sum)
                .unwrap()
                .unwrap();
            assert_eq!(now.values, [Scalar::Integer(7)].into_iter().collect());
            let copy = Store::open_from_bytes(&store.serialize_db().unwrap()).unwrap();
            assert_eq!(
                read_log(&copy, 0, &request.fluent, Policy::Sum).unwrap(),
                read_log(&store, 0, &request.fluent, Policy::Sum).unwrap()
            );
        });
    }
}

#[cfg(all(test, feature = "shacl"))]
mod admission_tests {
    use super::tests::{identity, loaded, request};
    use super::*;
    use crate::transaction_auth::with_identity;

    #[test]
    fn explicit_shacl_refusal_rolls_back_native_rows_and_recovers_term_cache() {
        let mut store = loaded();
        let before = store.latest_tx_id().unwrap();
        let deny = format!(
            "@prefix sh:<http://www.w3.org/ns/shacl#> . <urn:deny> a sh:NodeShape; sh:targetClass <{}FluentObservation>; sh:property [ sh:path <{}observationValue>; sh:maxCount 0 ] .",
            crate::namespace::QUIPU,
            crate::namespace::QUIPU
        );
        with_identity(Some(identity()), || {
            let result =
                store.with_write_shapes(Some(&deny), |s| s.append_fluent_native(&request(), 0));
            assert!(result.is_err(), "{result:?}");
            assert!(store.current_facts().unwrap().is_empty());
            assert_eq!(store.latest_tx_id().unwrap(), before);
            assert_eq!(store.lookup("urn:event:one").unwrap(), None);
            let tx = store.append_fluent_native(&request(), 0).unwrap();
            assert!(store.transaction_auth(tx).unwrap().is_some());
        });
    }

    #[test]
    fn graph_local_native_logs_preserve_their_own_binding() {
        let mut store = loaded();
        let graph = store.graph_create("urn:native-graph").unwrap();
        with_identity(Some(identity()), || {
            let mut request = request();
            store.append_fluent_native(&request, 0).unwrap();
            request.event = "urn:event:named".into();
            request.value = Scalar::Integer(11);
            assert!(
                store.append_fluent_native(&request, graph).is_err(),
                "native fluent identity rebound to another graph"
            );
            request.fluent = "urn:fluent:named".into();
            request.event = "urn:event:one".into();
            assert!(
                store.append_fluent_native(&request, graph).is_err(),
                "event identity reused in another graph"
            );
            request.event = "urn:event:named".into();
            store.append_fluent_native(&request, graph).unwrap();
            let root = read_log(&store, 0, "urn:fluent:count", Policy::Sum).unwrap();
            let named = read_log(&store, graph, &request.fluent, Policy::Sum).unwrap();
            assert_eq!(root.observations("urn:fluent:count").len(), 1);
            assert_eq!(named.observations(&request.fluent).len(), 1);
            assert_eq!(
                root.current("urn:fluent:count", Policy::Sum)
                    .unwrap()
                    .unwrap()
                    .values,
                [Scalar::Integer(3)].into_iter().collect()
            );
            assert_eq!(
                named
                    .current(&request.fluent, Policy::Sum)
                    .unwrap()
                    .unwrap()
                    .values,
                [Scalar::Integer(11)].into_iter().collect()
            );
        });
    }
}

#[cfg(all(test, feature = "shacl"))]
mod immutable_tests {
    use super::tests::{identity, loaded, request};
    use super::*;
    use crate::transaction_auth::with_identity;

    #[test]
    fn ordinary_write_cannot_retract_retype_replace_or_forge_fluent_records() {
        let mut store = loaded();
        with_identity(Some(identity()), || {
            let request = request();
            store.append_fluent_native(&request, 0).unwrap();
            let log = read_log(&store, 0, &request.fluent, Policy::Sum).unwrap();
            let event = store.lookup(&request.event).unwrap().unwrap();
            let field = store
                .entity_facts(event)
                .unwrap()
                .into_iter()
                .find(|f| {
                    store.resolve(f.attribute).unwrap()
                        == format!("{}observationValue", crate::namespace::QUIPU)
                })
                .unwrap();
            let mut retract = Datum {
                entity: event,
                attribute: field.attribute,
                value: field.value.clone(),
                valid_from: crate::time::now_iso(),
                valid_to: None,
                op: Op::Retract,
            };
            assert!(
                store
                    .transact(
                        &[retract.clone()],
                        &crate::time::now_iso(),
                        Some("declared-fluent-writer"),
                        Some("quipu:fluent")
                    )
                    .is_err(),
                "ordinary transaction retracted native observation"
            );
            retract.op = Op::Assert;
            retract.value = Value::Int(100);
            assert!(
                store
                    .transact(&[retract], &crate::time::now_iso(), None, None)
                    .is_err()
            );
            let forged = datum(
                &store,
                "urn:forged-event",
                crate::namespace::RDF_TYPE,
                &iri(&format!("{}FluentObservation", crate::namespace::QUIPU)).unwrap(),
                &crate::time::now_iso(),
            )
            .unwrap();
            assert!(
                store
                    .transact(&[forged], &crate::time::now_iso(), None, None)
                    .is_err()
            );
            let declaration = datum(
                &store,
                &request.fluent,
                &format!("{}currentValuePolicy", crate::namespace::QUIPU),
                &literal("max"),
                &crate::time::now_iso(),
            )
            .unwrap();
            assert!(
                store
                    .transact(&[declaration], &crate::time::now_iso(), None, None)
                    .is_err()
            );
            assert_eq!(
                read_log(&store, 0, &request.fluent, Policy::Sum).unwrap(),
                log
            );
            let ordinary = datum(
                &store,
                "urn:ordinary",
                "urn:status",
                &literal("open"),
                &crate::time::now_iso(),
            )
            .unwrap();
            store
                .transact(&[ordinary], &crate::time::now_iso(), None, None)
                .unwrap();
        });
    }
}

#[cfg(all(test, feature = "shacl"))]
mod term_tests {
    use super::tests::{identity, loaded, request};
    use super::*;
    use crate::transaction_auth::with_identity;

    #[test]
    fn typed_latest_and_set_replay_keep_rdf_identity() {
        for (name, policy) in [("latest", Policy::Latest), ("set", Policy::Set)] {
            let mut store = loaded();
            let declaration = format!(
                "<urn:count-shape> <http://www.w3.org/ns/shacl#path> <urn:count>; <http://www.w3.org/ns/shacl#targetNode> <urn:subject>; <{}currentValuePolicy> \"{name}\" .",
                crate::namespace::QUIPU
            );
            store
                .load_shapes("test-declaration", &declaration, "2026-11-08T02:01:00Z")
                .unwrap();
            with_identity(Some(identity()), || {
                let mut request = request();
                let values = [
                    Scalar::Integer(1),
                    Scalar::Term("\"01\"^^<http://www.w3.org/2001/XMLSchema#integer>".into()),
                    Scalar::Term("\"bonjour\"@fr".into()),
                    Scalar::Term("<urn:value>".into()),
                ];
                for (index, value) in values.into_iter().enumerate() {
                    request.event = format!("urn:event:term-{index}");
                    request.value = value;
                    let tx = store.append_fluent_native(&request, 0).unwrap();
                    assert_eq!(store.append_fluent_native(&request, 0).unwrap(), tx);
                    assert_eq!(store.latest_tx_id().unwrap(), tx);
                }
                let log = read_log(&store, 0, &request.fluent, policy).unwrap();
                assert_eq!(log.observations(&request.fluent).len(), 4);
                let current = log.current(&request.fluent, policy).unwrap().unwrap();
                assert_eq!(
                    current.values.len(),
                    if policy == Policy::Set { 4 } else { 1 }
                );
                assert!(log.observations(&request.fluent).iter().any(|o| o.value
                    == Scalar::Term("\"01\"^^<http://www.w3.org/2001/XMLSchema#integer>".into())));
            });
        }
    }
}

#[cfg(all(test, feature = "shacl", feature = "owl"))]
mod inference_tests {
    use super::tests::{identity, loaded, request};
    use super::*;
    use crate::transaction_auth::with_identity;

    #[test]
    fn native_append_refuses_inference_that_changes_prepared_record_content() {
        let mut store = loaded();
        store.load_ontology("record-domain", &format!("<{}writer> <http://www.w3.org/2000/01/rdf-schema#domain> <urn:UnexpectedRecordType> .", crate::namespace::QUIPU), "2026-11-08T03:00:00Z").unwrap();
        let before = store.latest_tx_id().unwrap();
        let result = with_identity(Some(identity()), || {
            store.append_fluent_native(&request(), 0)
        });
        assert!(
            result.is_err(),
            "inference changed immutable prepared event: {result:?}"
        );
        assert!(store.current_facts().unwrap().is_empty());
        assert_eq!(store.latest_tx_id().unwrap(), before);
    }

    #[test]
    fn native_append_allows_duplicate_record_type_and_unrelated_inferred_type() {
        let mut store = loaded();
        store.load_ontology("record-domain", &format!("<{}writer> <http://www.w3.org/2000/01/rdf-schema#domain> <{}FluentObservation>; <http://www.w3.org/2000/01/rdf-schema#range> <urn:WriterClass> .", crate::namespace::QUIPU, crate::namespace::QUIPU), "2026-11-08T03:00:00Z").unwrap();
        with_identity(Some(identity()), || {
            store.append_fluent_native(&request(), 0).unwrap();
            let log = read_log(&store, 0, &request().fluent, Policy::Sum).unwrap();
            assert_eq!(log.observations(&request().fluent).len(), 1);
            let writer = store.lookup(&identity().principal).unwrap().unwrap();
            let class = store.lookup("urn:WriterClass").unwrap().unwrap();
            assert!(
                store
                    .entity_facts(writer)
                    .unwrap()
                    .iter()
                    .any(|f| f.value == Value::Ref(class))
            );
        });
    }

    #[cfg(feature = "shacl")]
    #[test]
    fn primitive_declaration_limit_refuses_before_breaking_existing_native_reads() {
        let mut store = loaded();
        with_identity(Some(identity()), || {
            for i in 0..128 {
                let mut r = request();
                r.fluent = format!("urn:fluent:capacity:{i}");
                r.event = format!("urn:event:capacity:{i}");
                store.append_fluent_native(&r, 0).unwrap();
            }
            store
                .fluent_current_native(0, "urn:fluent:capacity:0")
                .unwrap();
            let tx = store.latest_tx_id().unwrap();
            let facts = store.current_facts().unwrap().len();
            let mut r = request();
            r.fluent = "urn:fluent:capacity:overflow".into();
            r.event = "urn:event:capacity:overflow".into();
            assert!(
                store.append_fluent_native(&r, 0).is_err(),
                "reject unsupported registry poststate before commit"
            );
            assert_eq!(store.latest_tx_id().unwrap(), tx);
            assert_eq!(store.current_facts().unwrap().len(), facts);
            store
                .fluent_current_native(0, "urn:fluent:capacity:0")
                .unwrap();
        });
    }
}
