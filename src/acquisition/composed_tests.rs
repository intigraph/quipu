use super::*;
use crate::{Op, Value, store::Datum};

const PROJECT: &str = "https://example.org/project/board";
const COLD: &str = "https://example.org/archive/cold";

fn write(store: &mut Store, graph: &str, text: &str, op: Op) -> i64 {
    let g = store.registered_graph_id(graph).unwrap().unwrap();
    let e = store
        .intern(if graph == COLD {
            "https://example.org/cold/item"
        } else {
            "https://example.org/live/item"
        })
        .unwrap();
    let a = store.intern("https://example.org/text").unwrap();
    store
        .transact_to_graph(
            &[Datum {
                entity: e,
                attribute: a,
                value: Value::Str(text.into()),
                valid_from: "2026-01-01T00:00:00Z".into(),
                valid_to: None,
                op,
            }],
            "2026-01-01T00:00:00Z",
            None,
            None,
            g,
        )
        .unwrap()
}

fn req(store: &Store, tx: Option<i64>) -> Request {
    Request {
        source_store_id: store.store_id().unwrap(),
        graphs: vec![PROJECT.into()],
        tx,
        expected_generation: None,
        max_bytes: None,
    }
}

fn main(dir: &std::path::Path) -> Store {
    let mut store = Store::open(dir.join("main.db").to_str().unwrap()).unwrap();
    store.graph_create(PROJECT).unwrap();
    write(&mut store, PROJECT, "before", Op::Assert);
    write(&mut store, PROJECT, "committed comment", Op::Assert);
    store
}

#[cfg(target_os = "linux")]
fn frozen(dir: &std::path::Path) -> Store {
    let mut store = main(dir);
    store.graph_create(COLD).unwrap();
    write(&mut store, COLD, "cold", Op::Assert);
    store
        .freeze_graph(COLD, dir.to_str().unwrap(), "2026-01-02T00:00:00Z", None)
        .unwrap();
    store
}

#[test]
fn main_direct_receipt_preserves_rdf_value_types_and_rejects_tamper() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = main(dir.path());
    let graph = store.registered_graph_id(PROJECT).unwrap().unwrap();
    let entity = store.intern("_:shared").unwrap();
    let attribute = store.intern("https://example.org/value").unwrap();
    let target = store.intern("https://example.org/target").unwrap();
    let values = [
        Value::Ref(target),
        Value::Int(-7),
        Value::Float(1.25),
        Value::Bool(true),
        Value::Lang {
            lexical: "hello@world".into(),
            lang: "en".into(),
        },
        Value::Typed {
            lexical: "2026-01-01".into(),
            datatype: "http://www.w3.org/2001/XMLSchema#date".into(),
        },
    ];
    let data: Vec<_> = values
        .into_iter()
        .map(|value| Datum {
            entity,
            attribute,
            value,
            valid_from: "2026-01-01T00:00:00Z".into(),
            valid_to: None,
            op: Op::Assert,
        })
        .collect();
    store
        .transact_to_graph(&data, "2026-01-01T00:00:00Z", None, None, graph)
        .unwrap();
    let related = "https://example.org/project/related";
    let other = store.graph_create(related).unwrap();
    store
        .transact_to_graph(
            &[Datum {
                entity,
                attribute,
                value: Value::Ref(entity),
                valid_from: "2026-01-01T00:00:00Z".into(),
                valid_to: None,
                op: Op::Assert,
            }],
            "2026-01-01T00:00:00Z",
            None,
            None,
            other,
        )
        .unwrap();
    let mut request = req(&store, None);
    request.graphs.push(related.into());
    let mut p = acquire(&store, &request).unwrap();
    p.verify(&request).unwrap();
    assert!(p.graphs[PROJECT].contains("committed comment"));
    assert_eq!(p.source_receipt.main.graphs[PROJECT].triples, 8);
    assert!(p.graphs[PROJECT].contains("_:shared"));
    assert!(p.graphs[related].contains("_:shared"));
    assert!(p.graphs[PROJECT].contains("\"hello@world\"@en"));
    assert!(p.graphs[PROJECT].contains("<https://example.org/target>"));
    p.graphs.get_mut(PROJECT).unwrap().push_str("garbage");
    assert!(p.verify(&request).is_err());
    let mut p = acquire(&store, &request).unwrap();
    p.source_receipt.composition.incarnation = "forged".into();
    assert!(p.verify(&request).is_err());
    let unsupported = Datum {
        entity,
        attribute,
        value: Value::Bytes(vec![0, 255]),
        valid_from: "2026-01-01T00:00:00Z".into(),
        valid_to: None,
        op: Op::Assert,
    };
    store
        .transact_to_graph(&[unsupported], "2026-01-01T00:00:00Z", None, None, graph)
        .unwrap();
    assert!(acquire(&store, &request).is_err());
}

#[test]
fn unknown_required_ephemeral_and_alias_scope_refuse_instead_of_empty() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = main(dir.path());
    let mut request = req(&store, None);
    request.graphs.push(format!("{PROJECT}#seeds-ephemeral"));
    assert!(acquire(&store, &request).is_err());
    store
        .dataset_create(
            PROJECT,
            &[crate::store::datasets::DatasetMember::new(PROJECT)],
            "2026-01-01T00:00:00Z",
            None,
        )
        .unwrap();
    assert!(acquire(&store, &req(&store, None)).is_err());
}

#[test]
fn cold_historical_topology_is_unknown_then_observed_pins_are_reusable() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = main(dir.path());
    let old = store.latest_tx_id().unwrap();
    write(&mut store, PROJECT, "extra", Op::Assert);
    assert!(acquire(&store, &req(&store, Some(old))).is_err());
    let first = acquire(&store, &req(&store, None)).unwrap();
    let pin = first.source_receipt.main.applied_tx;
    write(&mut store, PROJECT, "before", Op::Retract);
    write(&mut store, PROJECT, "after", Op::Assert);
    assert!(acquire(&store, &req(&store, Some(pin))).is_err());
    let mut historical_request = req(&store, Some(pin));
    historical_request.expected_generation =
        Some(first.source_receipt.composition.generation.clone());
    let historical = acquire(&store, &historical_request).unwrap();
    historical.verify(&historical_request).unwrap();
    assert!(historical.graphs[PROJECT].contains("before"));
    assert!(!historical.graphs[PROJECT].contains("after"));
    let now = acquire(&store, &req(&store, None)).unwrap();
    assert!(now.graphs[PROJECT].contains("after"));
    assert!(!now.graphs[PROJECT].contains("\"before\""));
}

#[test]
fn concurrent_main_writer_cannot_mix_receipt_and_payload() {
    let dir = tempfile::tempdir().unwrap();
    let store = main(dir.path());
    let head = store.latest_tx_id().unwrap();
    let p = acquire_with_control(&store, &req(&store, None), || {
        let mut writer = Store::open(dir.path().join("main.db").to_str().unwrap())?;
        write(&mut writer, PROJECT, "concurrent", Op::Assert);
        Ok(())
    })
    .unwrap();
    assert_eq!(p.source_receipt.main.applied_tx, head);
    assert!(!p.graphs[PROJECT].contains("concurrent"));
    assert!(acquire(&store, &req(&store, None)).unwrap().graphs[PROJECT].contains("concurrent"));
}

#[test]
fn strict_pins_byte_cap_census_and_generation_controls() {
    for tx in [
        serde_json::json!(null),
        serde_json::json!("1"),
        serde_json::json!(-1),
        serde_json::json!(1.1),
    ] {
        assert!(
            serde_json::from_value::<Request>(
                serde_json::json!({"source_store_id":"x","graphs":[PROJECT],"tx":tx})
            )
            .is_err()
        );
    }
    let dir = tempfile::tempdir().unwrap();
    let mut store = main(dir.path());
    let mut r = req(&store, None);
    r.max_bytes = Some(1);
    assert!(acquire(&store, &r).is_err());
    r.max_bytes = None;
    r.expected_generation = Some("foreign".into());
    assert!(acquire(&store, &r).is_err());
    r.expected_generation = None;
    r.tx = Some(i64::MAX);
    assert!(acquire(&store, &r).is_err());
    for n in 0..17 {
        store
            .attachments
            .push(crate::store::attach::Attachment::read_only(
                &format!("provider{n}"),
                "missing",
            ));
    }
    assert!(acquire(&store, &req(&store, None)).is_err());
}

#[cfg(target_os = "linux")]
#[test]
fn attached_main_positive_and_historical_before_retract_after() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = frozen(dir.path());
    let request = req(&store, None);
    assert!(
        crate::acquisition::acquire(
            &store,
            &crate::acquisition::Request {
                source_store_id: request.source_store_id.clone(),
                graphs: request.graphs.clone(),
                tx: None,
                max_bytes: None
            }
        )
        .is_err(),
        "v1 refusal retained"
    );
    let p = acquire(&store, &request).unwrap();
    p.verify(&request).unwrap();
    assert_eq!(p.source_receipt.composition.providers.len(), 1);
    assert!(p.graphs[PROJECT].contains("committed comment"));
    assert!(!p.graphs[PROJECT].contains("cold"));
    let pin = p.source_receipt.main.applied_tx;
    write(&mut store, PROJECT, "before", Op::Retract);
    write(&mut store, PROJECT, "after", Op::Assert);
    let mut r = req(&store, Some(pin));
    r.expected_generation = Some(p.source_receipt.composition.generation.clone());
    let old = acquire(&store, &r).unwrap();
    old.verify(&r).unwrap();
    assert!(old.graphs[PROJECT].contains("before"));
    assert!(!old.graphs[PROJECT].contains("after"));
    let now = acquire(&store, &req(&store, None)).unwrap();
    assert!(now.graphs[PROJECT].contains("after"));
    assert!(!now.graphs[PROJECT].contains("\"before\""));
    let mut r = req(&store, None);
    r.graphs = vec![COLD.into()];
    assert!(acquire(&store, &r).is_err());
}

#[cfg(target_os = "linux")]
#[test]
fn nonselected_provider_identity_alias_influence_refuses() {
    let dir = tempfile::tempdir().unwrap();
    let store = frozen(dir.path());
    let e = store.intern("https://example.org/live/item").unwrap();
    let cold: i64 = store
        .conn
        .query_row("SELECT id FROM main.terms WHERE iri=?1", [COLD], |r| {
            r.get(0)
        })
        .unwrap();
    store
        .conn
        .execute(
            "INSERT OR REPLACE INTO temp.term_alias(alias_id,canonical_id) VALUES(?1,?2)",
            rusqlite::params![e, cold],
        )
        .unwrap();
    assert!(acquire(&store, &req(&store, None)).is_err());
}

#[cfg(target_os = "linux")]
#[test]
fn existing_external_writer_and_new_writer_break_refuse_and_release_leases() {
    use std::os::unix::fs::OpenOptionsExt;
    let dir = tempfile::tempdir().unwrap();
    let store = frozen(dir.path());
    let path = store.attachments[0].path.clone();
    let writer = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
    assert!(acquire(&store, &req(&store, None)).is_err());
    drop(writer);
    let error = acquire_with_control(&store, &req(&store, None), || {
        assert!(
            std::fs::OpenOptions::new()
                .write(true)
                .custom_flags(libc::O_NONBLOCK)
                .open(&path)
                .is_err()
        );
        Ok(())
    })
    .unwrap_err();
    assert!(error.to_string().contains("lease broke"), "{error}");
    // No stale lease or pending SIGIO may survive either refusal.
    let writer = std::fs::OpenOptions::new()
        .write(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(&path)
        .unwrap();
    drop(writer);
    acquire(&store, &req(&store, None)).unwrap();
}

#[cfg(target_os = "linux")]
#[test]
fn mounted_provider_path_replacement_and_aba_refuse() {
    let dir = tempfile::tempdir().unwrap();
    let store = frozen(dir.path());
    let path = store.attachments[0].path.clone();
    let moved = format!("{path}.moved");
    assert!(
        acquire_with_control(&store, &req(&store, None), || {
            std::fs::rename(&path, &moved).unwrap();
            std::fs::write(&path, b"replacement").unwrap();
            Ok(())
        })
        .is_err()
    );
    std::fs::remove_file(&path).unwrap();
    std::fs::rename(&moved, &path).unwrap();
    // Even restored names are not an old provider generation.
    let first = acquire(&store, &req(&store, None)).unwrap();
    let mut r = req(&store, None);
    r.expected_generation = Some(first.source_receipt.composition.generation);
    assert!(
        acquire_with_control(&store, &r, || {
            std::fs::rename(&path, &moved).unwrap();
            std::fs::rename(&moved, &path).unwrap();
            Ok(())
        })
        .is_err()
    );
}

#[test]
fn expired_deadline_refuses_and_restores_positive_control() {
    let dir = tempfile::tempdir().unwrap();
    let store = main(dir.path());
    {
        let _guard =
            crate::time::set_request_deadline(Some(crate::time::Deadline::after_millis(0)));
        assert!(matches!(
            acquire(&store, &req(&store, None)),
            Err(Error::QueryTimeout { .. })
        ));
    }
    acquire(&store, &req(&store, None)).unwrap();
}

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
#[test]
fn provider_byte_cap_and_unmounted_frozen_registry_refuse() {
    let dir = tempfile::tempdir().unwrap();
    let store = frozen(dir.path());
    acquire(&store, &req(&store, None)).unwrap();
    let path = &store.attachments[0].path;
    let size = std::fs::metadata(path).unwrap().len();
    std::fs::OpenOptions::new()
        .write(true)
        .open(path)
        .unwrap()
        .set_len(PROVIDER_BYTES + 1)
        .unwrap();
    assert!(
        acquire(&store, &req(&store, None))
            .unwrap_err()
            .to_string()
            .contains("IO cap")
    );
    std::fs::OpenOptions::new()
        .write(true)
        .open(path)
        .unwrap()
        .set_len(size)
        .unwrap();
    acquire(&store, &req(&store, None)).unwrap();
    store
        .conn
        .execute(
            "UPDATE main.frozen_packs SET alias='missing_mount' WHERE thawed_at IS NULL",
            [],
        )
        .unwrap();
    assert!(acquire(&store, &req(&store, None)).is_err());
}

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
#[test]
fn provider_scope_added_before_lease_cannot_hide_in_mounted_cache() {
    let dir = tempfile::tempdir().unwrap();
    let store = frozen(dir.path());
    acquire(&store, &req(&store, None)).unwrap();
    {
        let writer = rusqlite::Connection::open(&store.attachments[0].path).unwrap();
        let id: i64 = writer
            .query_row("SELECT MAX(id)+1 FROM terms", [], |r| r.get(0))
            .unwrap();
        writer
            .execute(
                "INSERT INTO terms(id,iri) VALUES(?1,?2)",
                rusqlite::params![id, PROJECT],
            )
            .unwrap();
        writer
            .execute("INSERT INTO graphs(g,class,created_at) VALUES(?1,'committed','2026-01-01T00:00:00Z')", [id])
            .unwrap();
    }
    assert!(acquire(&store, &req(&store, None)).is_err());
}

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
#[test]
fn runtime_detach_remount_same_file_invalidates_old_generation() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = frozen(dir.path());
    let before = acquire(&store, &req(&store, None)).unwrap();
    store
        .conn
        .execute(
            "UPDATE frozen_packs SET thawed_at='2026-01-03T00:00:00Z'",
            [],
        )
        .unwrap();
    assert!(store.sync_frozen_attachments().unwrap());
    store
        .conn
        .execute("UPDATE frozen_packs SET thawed_at=NULL", [])
        .unwrap();
    assert!(store.sync_frozen_attachments().unwrap());
    let mut old = req(&store, None);
    old.expected_generation = Some(before.source_receipt.composition.generation.clone());
    assert!(
        acquire(&store, &old).is_err(),
        "runtime ABA cannot reuse an old generation"
    );
    let after = acquire(&store, &req(&store, None)).unwrap();
    assert_eq!(
        before.source_receipt.composition.providers,
        after.source_receipt.composition.providers
    );
    assert_ne!(
        before.source_receipt.composition.generation,
        after.source_receipt.composition.generation
    );
}

#[test]
fn label_dependency_change_invalidates_generation_and_timed_floor_refuses() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = main(dir.path());
    store.labels_config.min_freshness = Some("fresh".into());
    let graph = store.registered_graph_id(PROJECT).unwrap().unwrap();
    store
        .conn
        .execute("UPDATE graphs SET fresh_rank=2 WHERE g=?1", [graph])
        .unwrap();
    let before = acquire(&store, &req(&store, None)).unwrap();
    store
        .conn
        .execute("UPDATE graphs SET data_kind='source' WHERE g=?1", [graph])
        .unwrap();
    let mut old = req(&store, None);
    old.expected_generation = Some(before.source_receipt.composition.generation);
    assert!(acquire(&store, &old).is_err());
    acquire(&store, &req(&store, None)).unwrap();
    store
        .conn
        .execute(
            "UPDATE graphs SET labels_valid_to='2099-01-01T00:00:00Z' WHERE g=?1",
            [graph],
        )
        .unwrap();
    assert!(
        acquire(&store, &req(&store, None))
            .unwrap_err()
            .to_string()
            .contains("label floor is unsupported")
    );
}

#[test]
fn failed_first_acquisition_cannot_admit_an_earlier_history_floor() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = main(dir.path());
    let old_head = store.latest_tx_id().unwrap();
    let mut failed = req(&store, None);
    failed.max_bytes = Some(1);
    assert!(acquire(&store, &failed).is_err());
    let current_head = write(&mut store, PROJECT, "after refusal", Op::Assert);
    assert!(current_head > old_head);
    let first = acquire(&store, &req(&store, None)).unwrap();
    let mut old = req(&store, Some(old_head));
    old.expected_generation = Some(first.source_receipt.composition.generation.clone());
    let old_pin = acquire(&store, &old);
    println!(
        "ANCHORDEMO refused_max_bytes=1 old_head={old_head} first_success_head={current_head} history_floor={} old_pin_accepted={}",
        first.source_receipt.composition.history_min_tx,
        old_pin.is_ok()
    );
    assert_eq!(
        first.source_receipt.composition.history_min_tx, current_head,
        "only a successful fenced receipt can establish the history floor"
    );
    assert!(old_pin.is_err(), "refused observation admitted an old pin");
    old.tx = Some(current_head);
    acquire(&store, &old).unwrap().verify(&old).unwrap();
}

#[test]
fn failed_new_scopes_consume_no_anchor_slots_and_preserve_observed_history() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = main(dir.path());
    let observed = acquire(&store, &req(&store, None)).unwrap();
    let before = store.acquisition_anchors.borrow().clone();
    let mut last = req(&store, None);
    for n in 0..17 {
        let graph = format!("https://example.org/new/scope/{n}");
        store.graph_create(&graph).unwrap();
        write(&mut store, &graph, "new scope", Op::Assert);
        last = req(&store, None);
        last.graphs = vec![graph];
        last.max_bytes = Some(1);
        assert!(acquire(&store, &last).is_err());
        assert_eq!(*store.acquisition_anchors.borrow(), before);
    }
    last.max_bytes = None;
    let current = acquire(&store, &last).unwrap();
    assert_eq!(
        current.source_receipt.composition.history_min_tx,
        store.latest_tx_id().unwrap()
    );
    assert_eq!(store.acquisition_anchors.borrow().len(), 2);
    let mut old = req(&store, Some(observed.source_receipt.main.applied_tx));
    old.expected_generation = Some(observed.source_receipt.composition.generation);
    acquire(&store, &old).unwrap().verify(&old).unwrap();
}

#[test]
fn failed_generation_change_preserves_the_previously_admitted_anchor() {
    let dir = tempfile::tempdir().unwrap();
    let store = main(dir.path());
    acquire(&store, &req(&store, None)).unwrap();
    let before = store.acquisition_anchors.borrow().clone();
    // A harmless floor-policy change participates in the bound generation.
    let mut store = store;
    store
        .labels_config
        .deny_policy_tokens
        .push("unused-deny-token".into());
    let mut failed = req(&store, None);
    failed.max_bytes = Some(1);
    assert!(acquire(&store, &failed).is_err());
    assert_eq!(*store.acquisition_anchors.borrow(), before);
    let previous_generation = before.values().next().unwrap().0.clone();
    let current = acquire(&store, &req(&store, None)).unwrap();
    assert_ne!(
        current.source_receipt.composition.generation,
        previous_generation
    );
}
