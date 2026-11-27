//! M1 exit proof: one positive whole-set fork per owner kind, the X1-X5
//! negatives with nothing written, and a digest-construction sabotage control.
use super::*;
use crate::{
    graph_catalog::{Budget, GraphSet, WriteRequest},
    store::{Datum, graph_owner::GraphSetBatch},
    types::{Op, Value as TermValue},
};
use sha2::{Digest, Sha256};
use std::{path::Path, sync::Arc};

pub(super) const DATA: &str = "urn:application";
pub(super) const PROV: &str = "urn:provenance";
pub(super) const LEG: &str = "urn:legacy-notes";
pub(super) const FROM: &str = "2026-01-01T00:00:00Z";

pub(super) struct Home {
    pub(super) _dir: tempfile::TempDir,
    pub(super) owner: GraphSetOwner,
    pub(super) legacy: Store,
    pub(super) catalog: Arc<GraphSetCatalog>,
    pub(super) routed_id: String,
    pub(super) legacy_id: String,
}

fn adopted(root: &Path, file: &str, graphs: &[&str]) -> Store {
    let path = root.join(file);
    {
        let store = Store::open(path.to_str().unwrap()).unwrap();
        for g in graphs {
            store.graph_create(g).unwrap();
        }
    }
    Store::adopt(path.to_str().unwrap()).unwrap();
    Store::open(path.to_str().unwrap()).unwrap()
}

fn route(graph: &str, file: &str, store: &Store, generation: u64) -> Route {
    Route {
        graph: graph.into(),
        file_id: file.into(),
        store_id: store.store_id().unwrap(),
        log_epoch: store.log_epoch().unwrap(),
        generation,
        policy: "policy-v1".into(),
    }
}

pub(super) fn home() -> Home {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let store = adopted(root, "application.db", &[DATA, PROV]);
    let mut legacy = adopted(root, "legacy.db", &[LEG]);
    let datum = |s: &Store, v: &str| Datum {
        entity: s.intern("urn:item").unwrap(),
        attribute: s.intern("urn:value").unwrap(),
        value: TermValue::Str(v.into()),
        valid_from: FROM.into(),
        valid_to: None,
        op: Op::Assert,
    };
    let leg_g = legacy.committed_graph_id(LEG).unwrap().unwrap();
    let leg_datum = datum(&legacy, "noted");
    legacy
        .ownership_operation(|s| {
            s.transact_graph_batches_tx(&[(leg_g, vec![leg_datum])], FROM, None, None)
        })
        .unwrap();
    let (routed, legacy_id) = (
        route(DATA, "application", &store, 3),
        legacy.store_id().unwrap(),
    );
    let batch = GraphSetBatch {
        store_id: routed.store_id.clone(),
        log_epoch: routed.log_epoch.clone(),
        graphs: vec![
            (DATA.into(), vec![datum(&store, "present")]),
            (PROV.into(), vec![datum(&store, "recorded")]),
        ],
        timestamp: FROM.into(),
        actor: Some("fixture".into()),
        source: Some("fixture".into()),
    };
    let catalog = Arc::new(
        GraphSetCatalog::new(
            7,
            &legacy_id,
            vec![GraphSet {
                owner: routed.clone(),
                graphs: vec![DATA.into(), PROV.into()],
            }],
            Budget {
                max_owners: 1,
                queued_per_owner: 1,
                readers_per_owner: 1,
                max_queued: 1,
                max_connections: 8,
            },
            8,
        )
        .unwrap()
        .with_legacy_graphs(
            route(ROOT_GRAPH_IRI, "legacy", &legacy, 1),
            vec![ROOT_GRAPH_IRI.into(), LEG.into()],
        )
        .unwrap(),
    );
    let allocation = catalog.physical().memory_allocation(512, 3, 1).unwrap();
    let owner = GraphSetOwner::prepare(catalog.clone(), root, DATA, store, "policy-v1", allocation)
        .unwrap();
    let requests: Vec<_> = [DATA, PROV]
        .into_iter()
        .map(|graph| WriteRequest {
            graph: Some(graph),
            catalog_revision: 7,
            owner_generation: Some(3),
            policy: "policy-v1",
            signed: false,
            cross_graph_context: false,
        })
        .collect();
    owner.transact(&requests, &batch).unwrap();
    Home {
        _dir: dir,
        owner,
        legacy,
        catalog,
        routed_id: routed.store_id,
        legacy_id,
    }
}

fn request(members: &[(&str, MemberRole)]) -> MiniForkRequest {
    MiniForkRequest {
        name: "agent-a".into(),
        store_id: "urn:uuid:00000000-0000-4000-8000-00000000a11c".into(),
        log_epoch: "urn:uuid:00000000-0000-4000-8000-0000000e90c4".into(),
        members: members
            .iter()
            .map(|(iri, role)| MiniForkMember {
                iri: (*iri).into(),
                role: *role,
            })
            .collect(),
        created_at: FROM.into(),
    }
}

use MemberRole::{Reference, Writable};

/// Independent construction: canonical N-Quad + valid interval, sorted, JSON,
/// sha256. Written out here so the implementation cannot pass by agreeing
/// with itself.
fn expected_digest(lines: &[(&str, &str)]) -> String {
    let mut rows: Vec<(String, String, Option<String>)> = lines
        .iter()
        .map(|(value, graph)| {
            (
                format!("<urn:item> <urn:value> \"{value}\" <{graph}> ."),
                FROM.to_owned(),
                None,
            )
        })
        .collect();
    rows.sort();
    format!(
        "sha256:{}",
        hex::encode(Sha256::digest(serde_json::to_vec(&rows).unwrap()))
    )
}

fn nothing_written(home: &Home, routed_tx: i64, legacy_tx: i64) {
    assert_eq!(home.owner.owner.read(mini_fork_count).unwrap(), 0);
    assert_eq!(mini_fork_count(&home.legacy).unwrap(), 0);
    assert_eq!(
        home.owner.owner.read(Store::latest_tx_id).unwrap(),
        routed_tx
    );
    assert_eq!(home.legacy.latest_tx_id().unwrap(), legacy_tx);
}

#[test]
fn whole_set_fork_from_one_routed_owner_records_the_expected_cut_and_digest() {
    let home = home();
    let head = home.owner.owner.read(Store::latest_tx_id).unwrap();
    let req = request(&[(PROV, Writable), (DATA, Writable), (LEG, Reference)]);
    let entry = home.owner.record_mini_fork(&req).unwrap();
    assert_eq!(
        entry.base,
        BaseCut {
            store_id: home.routed_id.clone(),
            log_epoch: home.owner.owner.read(Store::log_epoch).unwrap(),
            local_tx: head,
            catalog_revision: 7,
            owner_generation: 3,
            policy_revision: "policy-v1".into(),
        }
    );
    assert_eq!(entry.writable, vec![DATA.to_owned(), PROV.to_owned()]);
    assert_eq!(
        entry.references,
        vec![ReferenceMember {
            iri: LEG.into(),
            owner_store_id: home.legacy_id.clone()
        }]
    );
    assert_eq!(
        (entry.store_id.as_str(), entry.name.as_str()),
        (req.store_id.as_str(), "agent-a")
    );
    assert_eq!(
        entry.base_digest,
        expected_digest(&[("present", DATA), ("recorded", PROV)])
    );
    assert_eq!(entry.shapes_hash, crate::client_changeset::digest(b""));
    assert_eq!(home.owner.mini_fork("agent-a").unwrap(), Some(entry));
    // The entry is a control write: no member graph transaction was added,
    // and the catalog still routes exactly what it routed before.
    assert_eq!(home.owner.owner.read(Store::latest_tx_id).unwrap(), head);
    assert_eq!(home.catalog.owner_graphs(DATA).unwrap(), vec![DATA, PROV]);
}

#[test]
fn legacy_fork_digest_equals_the_local_sync_base_slice_hash() {
    let mut home = home();
    let head = home.legacy.latest_tx_id().unwrap();
    let entry = record_on_legacy(
        &home.catalog,
        &mut home.legacy,
        &request(&[(LEG, Writable)]),
    )
    .unwrap();
    assert_eq!(entry.base.store_id, home.legacy_id);
    assert_eq!(
        (entry.base.local_tx, entry.base.owner_generation),
        (head, 1)
    );
    let journal = member_journal(&home.legacy, head, LEG).unwrap();
    assert_eq!(entry.base_digest, journal.slice_hash().unwrap());
    assert_eq!(entry.base_digest, expected_digest(&[("noted", LEG)]));
    assert_eq!(
        mini_fork_entry(&home.legacy, "agent-a").unwrap(),
        Some(entry)
    );
}

/// Sabotage control: each of these is a plausible different construction.
/// If `base_digest` were computed over any of them the positive digest
/// assertion above fails, and this test fails if any of them collides.
#[test]
fn sabotage_a_different_digest_construction_is_detected() {
    let home = home();
    let entry = home
        .owner
        .record_mini_fork(&request(&[(DATA, Writable), (PROV, Writable)]))
        .unwrap();
    let journals: Vec<_> = home
        .owner
        .owner
        .read(|s| {
            [DATA, PROV]
                .iter()
                .map(|g| member_journal(s, entry.base.local_tx, g))
                .collect()
        })
        .unwrap();
    let hash = |v: &serde_json::Value| {
        format!(
            "sha256:{}",
            hex::encode(Sha256::digest(serde_json::to_vec(v).unwrap()))
        )
    };
    // Per-graph slice hashes chained instead of one sorted line set.
    let chained: Vec<_> = journals.iter().map(|j| j.slice_hash().unwrap()).collect();
    // Lines without their valid interval.
    let no_interval = serde_json::json!([
        "<urn:item> <urn:value> \"present\" <urn:application> .",
        "<urn:item> <urn:value> \"recorded\" <urn:provenance> ."
    ]);
    // Triples without the graph term.
    let no_graph = serde_json::json!([
        ["<urn:item> <urn:value> \"present\" .", FROM, null],
        ["<urn:item> <urn:value> \"recorded\" .", FROM, null]
    ]);
    let wrong = [
        hash(&serde_json::json!(chained)),
        hash(&no_interval),
        hash(&no_graph),
        journals[0].slice_hash().unwrap(),
    ];
    assert_eq!(entry.base_digest, base_digest(&journals).unwrap());
    for candidate in wrong {
        assert_ne!(
            entry.base_digest, candidate,
            "digest matched a different construction"
        );
    }
}

#[test]
fn x2_partial_catalog_set_refuses_with_nothing_written() {
    let home = home();
    let (r, l) = (
        home.owner.owner.read(Store::latest_tx_id).unwrap(),
        home.legacy.latest_tx_id().unwrap(),
    );
    let err = home
        .owner
        .record_mini_fork(&request(&[(DATA, Writable), (PROV, Reference)]))
        .unwrap_err()
        .to_string();
    assert!(err.contains("X2 partial-catalog-set"), "{err}");
    nothing_written(&home, r, l);
}

#[test]
fn x3_prefix_or_pattern_member_refuses_with_nothing_written() {
    let home = home();
    let (r, l) = (
        home.owner.owner.read(Store::latest_tx_id).unwrap(),
        home.legacy.latest_tx_id().unwrap(),
    );
    for member in [
        "urn:application*",
        "https://example.org/graphs/",
        "urn:week-{n}",
    ] {
        let err = home
            .owner
            .record_mini_fork(&request(&[
                (DATA, Writable),
                (PROV, Writable),
                (member, Reference),
            ]))
            .unwrap_err()
            .to_string();
        assert!(err.contains("X3 prefix-or-pattern-member"), "{err}");
    }
    nothing_written(&home, r, l);
}

#[test]
fn x4_multi_file_set_refuses_with_nothing_written() {
    let mut home = home();
    let (r, l) = (
        home.owner.owner.read(Store::latest_tx_id).unwrap(),
        home.legacy.latest_tx_id().unwrap(),
    );
    let req = request(&[(DATA, Writable), (PROV, Writable), (LEG, Writable)]);
    for err in [
        home.owner.record_mini_fork(&req).unwrap_err().to_string(),
        record_on_legacy(&home.catalog, &mut home.legacy, &req)
            .unwrap_err()
            .to_string(),
    ] {
        assert!(err.contains("X4 multi-file-set"), "{err}");
    }
    nothing_written(&home, r, l);
}

#[test]
fn x1_routed_writer_claim_refuses_with_nothing_written() {
    let mut home = home();
    let (r, l) = (
        home.owner.owner.read(Store::latest_tx_id).unwrap(),
        home.legacy.latest_tx_id().unwrap(),
    );
    for claimed in [home.routed_id.clone(), home.legacy_id.clone()] {
        let mut req = request(&[(DATA, Writable), (PROV, Writable)]);
        req.store_id = claimed.clone();
        let err = home.owner.record_mini_fork(&req).unwrap_err().to_string();
        assert!(err.contains("X1 routed-writer-claim"), "{err}");
        let mut req = request(&[(LEG, Writable)]);
        req.store_id = claimed;
        let err = record_on_legacy(&home.catalog, &mut home.legacy, &req)
            .unwrap_err()
            .to_string();
        assert!(err.contains("X1 routed-writer-claim"), "{err}");
    }
    nothing_written(&home, r, l);
}

#[test]
fn x5_fork_from_a_mini_quipu_refuses_with_nothing_written() {
    let mut home = home();
    mark_mini_quipu(
        &home.legacy,
        "urn:uuid:00000000-0000-4000-8000-0000000000e0",
    )
    .unwrap();
    let (r, l) = (
        home.owner.owner.read(Store::latest_tx_id).unwrap(),
        home.legacy.latest_tx_id().unwrap(),
    );
    let err = record_on_legacy(
        &home.catalog,
        &mut home.legacy,
        &request(&[(LEG, Writable)]),
    )
    .unwrap_err()
    .to_string();
    assert!(err.contains("X5 fork-from-mini-quipu"), "{err}");
    nothing_written(&home, r, l);
}

#[test]
fn a_registered_name_or_identity_is_not_recorded_twice() {
    let home = home();
    let req = request(&[(DATA, Writable), (PROV, Writable)]);
    home.owner.record_mini_fork(&req).unwrap();
    let err = home.owner.record_mini_fork(&req).unwrap_err().to_string();
    assert!(err.contains("already registered"), "{err}");
    assert_eq!(home.owner.owner.read(mini_fork_count).unwrap(), 1);
}
