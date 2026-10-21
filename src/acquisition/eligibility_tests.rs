use std::sync::{Arc, Mutex};

use rusqlite::hooks::{AuthAction, AuthContext, Authorization};

use super::*;
use crate::{
    store::Datum,
    types::{Op, Value},
};

const PROJECT: &str = "https://example.org/project/board";
const COMMENTS: &str = "https://example.org/project/comments";

fn seed() -> Store {
    let mut store = Store::open_in_memory().unwrap();
    for iri in [PROJECT, COMMENTS] {
        let graph = store.graph_create(iri).unwrap();
        let datums = [Datum {
            entity: store.intern("https://example.org/item/one").unwrap(),
            attribute: store.intern("https://example.org/text").unwrap(),
            value: Value::Str("text".into()),
            valid_from: "2026-01-01T00:00:00Z".into(),
            valid_to: None,
            op: Op::Assert,
        }];
        store
            .transact_to_graph(&datums, "2026-01-01T00:00:00Z", None, None, graph)
            .unwrap();
    }
    store
}

fn request(store: &Store, graphs: &[&str]) -> Request {
    Request {
        source_store_id: store.store_id().unwrap(),
        graphs: graphs.iter().map(|g| (*g).to_string()).collect(),
        expected_head_tx: None,
    }
}

/// Every table action `work` performs, as `read:<table>` or `write:<table>`.
fn audit<T>(store: &Store, work: impl FnOnce() -> T) -> (T, Vec<String>) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&seen);
    store
        .conn
        .authorizer(Some(move |context: AuthContext<'_>| {
            let entry = match context.action {
                AuthAction::Read { table_name, .. } => Some(format!("read:{table_name}")),
                AuthAction::Insert { table_name }
                | AuthAction::Delete { table_name }
                | AuthAction::Update { table_name, .. } => Some(format!("write:{table_name}")),
                _ => None,
            };
            if let Some(entry) = entry {
                sink.lock().unwrap().push(entry);
            }
            Authorization::Allow
        }))
        .unwrap();
    let out = work();
    store
        .conn
        .authorizer(None::<fn(AuthContext<'_>) -> Authorization>)
        .unwrap();
    let mut seen = seen.lock().unwrap().clone();
    seen.sort();
    seen.dedup();
    (out, seen)
}

/// The eligibility path's whole table footprint: metadata only.
const METADATA_TABLES: [&str; 5] = [
    "read:datasets",
    "read:graphs",
    "read:store_identity",
    "read:terms",
    "read:transactions",
];

fn assert_metadata_only(seen: &[String]) {
    for entry in seen {
        assert!(
            METADATA_TABLES.contains(&entry.as_str()),
            "eligibility touched {entry}; all: {seen:?}"
        );
    }
}

fn status(observation: &Observation, iri: &str) -> Status {
    observation.graphs[iri].status.clone()
}

#[test]
fn an_eligible_scope_reads_only_metadata_and_writes_nothing() {
    let store = seed();
    let head = store.latest_tx_id().unwrap();
    let anchors = store.acquisition_anchors.borrow().clone();
    let incarnation = store.acquisition_incarnation.clone();
    let (observation, seen) = audit(&store, || {
        observe(&store, &request(&store, &[PROJECT, COMMENTS])).unwrap()
    });
    assert_eq!(seen, METADATA_TABLES.map(String::from).to_vec());
    assert_eq!(observation.schema, CONTRACT);
    assert_eq!(observation.authority, AUTHORITY);
    assert_eq!(observation.observed_head_tx, head);
    assert!(observation.policy.floors.is_empty());
    assert_eq!(status(&observation, PROJECT), Status::Eligible);
    assert_eq!(status(&observation, COMMENTS), Status::Eligible);
    assert_eq!(*store.acquisition_anchors.borrow(), anchors);
    assert_eq!(store.acquisition_incarnation, incarnation);
    assert_eq!(store.latest_tx_id().unwrap(), head);
    let value = serde_json::to_value(&observation).unwrap();
    for key in [
        "applied_tx",
        "requested_tx",
        "source_receipt",
        "complete",
        "graphs_sha256",
    ] {
        assert!(
            value.get(key).is_none(),
            "an observation must not carry {key}"
        );
    }
}

/// Positive control for the audit: a real acquisition of the same scope
/// reads facts, so an empty fact footprint above is a measurement.
#[test]
fn the_audit_sees_fact_reads_when_they_happen() {
    let store = seed();
    let acquisition = crate::acquisition::Request {
        source_store_id: store.store_id().unwrap(),
        graphs: vec![PROJECT.into()],
        tx: None,
        max_bytes: None,
    };
    let (payload, seen) = audit(&store, || crate::acquisition::acquire(&store, &acquisition));
    payload.unwrap();
    assert!(seen.iter().any(|e| e == "read:facts"), "{seen:?}");
}

#[test]
fn a_configured_floor_is_evaluated_from_metadata_alone() {
    let mut store = seed();
    let unset = observe(&store, &request(&store, &[PROJECT]))
        .unwrap()
        .policy;
    store.labels_config.min_freshness = Some("fresh".into());
    let graph = store.registered_graph_id(PROJECT).unwrap().unwrap();
    store
        .conn
        .execute("UPDATE graphs SET fresh_rank=2 WHERE g=?1", [graph])
        .unwrap();
    let (observation, seen) = audit(&store, || {
        observe(&store, &request(&store, &[PROJECT, COMMENTS])).unwrap()
    });
    assert_metadata_only(&seen);
    assert_eq!(status(&observation, PROJECT), Status::Eligible);
    assert_eq!(status(&observation, COMMENTS), Status::Refused);
    assert!(
        observation.graphs[COMMENTS]
            .reason
            .as_deref()
            .unwrap()
            .contains("freshness")
    );
    assert_eq!(observation.policy.floors, ["freshness"]);
    assert_ne!(observation.policy.sha256, unset.sha256);
}

#[test]
fn a_trust_or_time_floor_refuses_without_reading_labels_or_facts() {
    let mut store = seed();
    store.labels_config.min_trust_rank = Some(30);
    store.labels_config.min_trust_chain = Some("urn:chain".into());
    let graph = store.registered_graph_id(PROJECT).unwrap().unwrap();
    store
        .conn
        .execute("UPDATE graphs SET trust_rank=99 WHERE g=?1", [graph])
        .unwrap();
    let (observation, seen) = audit(&store, || {
        observe(&store, &request(&store, &[PROJECT])).unwrap()
    });
    assert_metadata_only(&seen);
    assert_eq!(status(&observation, PROJECT), Status::Refused);
    assert!(
        observation.graphs[PROJECT]
            .reason
            .as_deref()
            .unwrap()
            .contains("unsupported")
    );
    assert_eq!(observation.policy.floors, ["trust"]);
}

#[test]
fn policy_identity_is_a_digest_not_the_configured_values() {
    let mut store = seed();
    store
        .labels_config
        .deny_policy_tokens
        .push("private-token-value".into());
    let first = observe(&store, &request(&store, &[PROJECT])).unwrap();
    let text = serde_json::to_string(&first).unwrap();
    assert!(!text.contains("private-token-value"), "{text}");
    assert_eq!(first.policy.floors, ["policy_tokens"]);
    store.labels_config.deny_policy_tokens = vec!["other-token-value".into()];
    let second = observe(&store, &request(&store, &[PROJECT])).unwrap();
    assert_ne!(first.policy.sha256, second.policy.sha256);
}

#[test]
fn an_unregistered_graph_is_refused_and_does_not_fail_the_rest() {
    let store = seed();
    let observation = observe(
        &store,
        &request(&store, &[PROJECT, "https://example.org/project/absent"]),
    )
    .unwrap();
    assert_eq!(status(&observation, PROJECT), Status::Eligible);
    assert_eq!(
        status(&observation, "https://example.org/project/absent"),
        Status::Refused
    );
}

#[test]
fn a_failed_check_is_unknown_never_eligible() {
    let store = seed();
    store.conn.execute_batch("DROP TABLE datasets").unwrap();
    let observation = observe(&store, &request(&store, &[PROJECT])).unwrap();
    assert_eq!(status(&observation, PROJECT), Status::Unknown);
    assert!(observation.graphs[PROJECT].reason.is_some());
}

#[test]
fn malformed_or_mismatched_requests_refuse_whole() {
    let store = seed();
    let head = store.latest_tx_id().unwrap();
    let mut duplicate = request(&store, &[PROJECT, PROJECT]);
    assert!(observe(&store, &duplicate).is_err());
    duplicate.graphs.clear();
    assert!(observe(&store, &duplicate).is_err());
    let many: Vec<String> = (0..17)
        .map(|i| format!("https://example.org/g/{i}"))
        .collect();
    let refs: Vec<&str> = many.iter().map(String::as_str).collect();
    assert!(observe(&store, &request(&store, &refs)).is_err());
    assert!(observe(&store, &request(&store, &["not an iri"])).is_err());
    let mut lineage = request(&store, &[PROJECT]);
    lineage.source_store_id = "urn:uuid:other".into();
    assert!(observe(&store, &lineage).is_err());
    let mut stale = request(&store, &[PROJECT]);
    stale.expected_head_tx = Some(head - 1);
    assert!(observe(&store, &stale).is_err());
    let mut current = request(&store, &[PROJECT]);
    current.expected_head_tx = Some(head);
    assert_eq!(observe(&store, &current).unwrap().observed_head_tx, head);
}

#[test]
fn a_store_with_no_committed_transaction_has_no_head_to_observe() {
    let store = Store::open_in_memory().unwrap();
    assert_eq!(
        store.latest_tx_id().unwrap(),
        0,
        "precondition: nothing committed"
    );
    let error = observe(&store, &request(&store, &[PROJECT])).unwrap_err();
    assert!(
        error.to_string().contains("no committed transaction"),
        "{error}"
    );
}
