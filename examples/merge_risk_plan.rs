//! Offline paired demonstration: legacy inactive behavior and configured refusal.
use quipu::merge_plan::{Policy, plan, verify};
use quipu::store::{Datum, Store};
use quipu::types::{Op, Value};
use std::collections::{BTreeMap, BTreeSet};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let configured = std::env::args().any(|s| s == "--configured");
    let policy = Policy {
        threshold: 40,
        protected_classes: BTreeSet::from(["urn:Credential".into()]),
        protected_terms: BTreeSet::from(["urn:value".into()]),
        recall_queries: Some(BTreeMap::new()),
    };
    let mut store = Store::open_in_memory()?;
    let candidate = "<urn:demo> <urn:safe> \"value\" .";
    let report = plan(&store, "", candidate, &policy)?;
    println!(
        "root-only plan: {:?}, score={}",
        report.risk.verdict, report.risk.score
    );
    let mut evidence = serde_json::to_value(&report)?;
    verify(&store, "", candidate, &policy, &evidence)?;
    println!("same receiver verifies original proof: PASS");
    evidence["risk"]["score"] = serde_json::json!(99);
    assert!(verify(&store, "", candidate, &policy, &evidence).is_err());
    println!("same receiver rejects mutated proof: PASS");
    store.transact(&[], "2026-01-01T00:00:00Z", None, None)?;
    let fork = store.fork_create("demo", store.latest_tx_id()?, "2026-01-01T00:00:00Z", None)?;
    let datum = Datum {
        entity: store.intern("urn:demo")?,
        attribute: store.intern("urn:value")?,
        value: Value::Str("private fixture".into()),
        valid_from: "2026-01-01T00:00:00Z".into(),
        valid_to: None,
        op: Op::Assert,
    };
    store.transact_to_graph(&[datum], "2026-01-01T00:00:00Z", None, None, fork.g)?;
    let head = store.latest_tx_id()?;
    let outcome = store.fork_promote("demo", "2026-01-02T00:00:00Z", None);
    if configured {
        let error = outcome.expect_err("configured policy must hold protected data");
        assert!(error.to_string().contains("needs decisions"));
        assert_eq!(store.latest_tx_id()?, head);
        println!("configured actual fork promotion: REFUSED; ROOT unchanged");
    } else {
        outcome?;
        assert!(store.latest_tx_id()? > head);
        println!("inactive actual fork promotion: APPLIED; existing behavior retained");
    }
    println!("all stores were private and in memory; no deployment policy was installed");
    Ok(())
}
