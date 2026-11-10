use super::*;
use crate::merge_session::{Path, Profile, Request};
use crate::types::Value;
use std::collections::BTreeMap;

const T: &str = "2026-01-01T00:00:00Z";
const TD: &str = "2026-01-02T00:00:00Z";

fn configured() -> Policy {
    Policy {
        threshold: 40,
        protected_classes: BTreeSet::from(["urn:Credential".into()]),
        protected_terms: BTreeSet::from(["urn:p".into()]),
        recall_queries: Some(BTreeMap::new()),
    }
}

fn legacy(store: &mut Store, predicate: &str) -> Result<i64> {
    let before = crate::share_merge::root_graph(store)?.0;
    let incoming =
        crate::share_merge::parse_graph(&format!("<urn:a> <{predicate}> \"v\" ."), "fixture")?;
    let plan = crate::share_merge::core::plan(
        store,
        &crate::share_merge::core::Inputs {
            base: &before,
            ours: &before,
            theirs: &incoming,
            shapes: "",
            authored_at: T,
        },
        &BTreeMap::new(),
        TD,
    )?;
    crate::share_merge::core::atomic(store, |store| {
        crate::share_merge::core::persist(store, &plan, TD, None, "fixture")
    })
}

#[test]
fn configured_boundaries() {
    if std::env::var_os("QUIPU_MERGE_PLAN_TEST_CHILD").is_none() {
        let result = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "merge_plan_gate::tests::configured_boundaries",
                "--nocapture",
            ])
            .env("QUIPU_MERGE_PLAN_TEST_CHILD", "1")
            .env(POLICY_ENV, serde_json::to_string(&configured()).unwrap())
            .status()
            .unwrap();
        assert!(result.success());
        return;
    }
    // Real configured legacy share boundary, paired with a positive apply.
    let mut store = Store::open_in_memory().unwrap();
    let tx = store.latest_tx_id().unwrap();
    assert!(
        legacy(&mut store, "urn:p")
            .unwrap_err()
            .to_string()
            .contains("needs decisions")
    );
    assert_eq!(store.latest_tx_id().unwrap(), tx);
    assert!(crate::share_merge::root_graph(&store).unwrap().0.is_empty());
    legacy(&mut store, "urn:q").unwrap();
    assert_eq!(crate::share_merge::root_graph(&store).unwrap().0.len(), 1);

    // Overlay-only operations cannot be projected as ROOT retractions.
    let datum = Datum {
        entity: store.intern("urn:a").unwrap(),
        attribute: store.intern("urn:q").unwrap(),
        value: Value::Str("v".into()),
        valid_from: T.into(),
        valid_to: None,
        op: Op::Tombstone,
    };
    let tx = store.latest_tx_id().unwrap();
    let error = datums(&store, &[&[datum]], None).unwrap_err();
    assert!(error.to_string().contains("Tombstone"));
    assert_eq!(store.latest_tx_id().unwrap(), tx);
    assert_eq!(crate::share_merge::root_graph(&store).unwrap().0.len(), 1);

    // Actual profiled receive/decision/receipt boundary, before any receive.
    let mut store = Store::open_in_memory().unwrap();
    let tx = store.latest_tx_id().unwrap();
    let request = Request {
        store_id: store.store_id().unwrap(),
        root_tx: tx,
        path: Path::Share,
        input_id: "incoming".into(),
        base_id: "base".into(),
        base: String::new(),
        ours: String::new(),
        theirs: "<urn:a> <urn:p> \"v\" .\n".into(),
        shapes: String::new(),
        profile: Profile::default(),
        parent: None,
        active_slots: None,
        scope: None,
    };
    let prepared = crate::merge_adapters::prepare_store(&store, request, &[], T, TD).unwrap();
    let id = prepared.id.clone();
    let error =
        crate::merge_pending::execute(&mut store, "urn:ledger", prepared, T, TD, None).unwrap_err();
    assert!(error.to_string().contains("needs decisions"), "{error}");
    assert_eq!(store.latest_tx_id().unwrap(), tx);
    assert!(
        crate::merge_pending::load(&store, "urn:ledger", &id)
            .unwrap()
            .is_none()
    );

    // Normal legacy fork promotion, with an actual fork transaction.
    let mut store = Store::open_in_memory().unwrap();
    store.transact(&[], T, None, None).unwrap();
    let fork = store
        .fork_create("risk", store.latest_tx_id().unwrap(), T, None)
        .unwrap();
    let datum = Datum {
        entity: store.intern("urn:a").unwrap(),
        attribute: store.intern("urn:p").unwrap(),
        value: Value::Str("v".into()),
        valid_from: T.into(),
        valid_to: None,
        op: Op::Assert,
    };
    store
        .transact_to_graph(&[datum], T, None, None, fork.g)
        .unwrap();
    let tx = store.latest_tx_id().unwrap();
    let error = store.fork_promote("risk", TD, None).unwrap_err();
    assert!(error.to_string().contains("needs decisions"), "{error}");
    assert_eq!(store.latest_tx_id().unwrap(), tx);
    assert!(crate::share_merge::root_graph(&store).unwrap().0.is_empty());
}
