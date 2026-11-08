//! Actual execution probes; no fabricated observer records or validator inputs.
use super::*;
use crate::{Datum, Op, Value};
const TS: &str = "2026-01-01T00:00:00Z";
const SHAPES: &str = "@prefix sh:<http://www.w3.org/ns/shacl#>. <urn:S> a sh:NodeShape; sh:targetSubjectsOf <urn:p>; sh:property [sh:path <urn:p>; sh:maxCount 1].";
fn datum(store: &Store, subject: &str, predicate: &str, value: Value) -> Datum {
    Datum {
        entity: store.intern(subject).unwrap(),
        attribute: store.intern(predicate).unwrap(),
        value,
        valid_from: TS.into(),
        valid_to: None,
        op: Op::Assert,
    }
}
fn put(store: &mut Store, value: &str) -> Result<i64> {
    let d = datum(store, "urn:e", "urn:p", Value::Str(value.into()));
    store.transact(&[d], TS, None, None)
}
fn one(observations: &[ValidatorObservation]) -> &ValidatorObservation {
    assert_eq!(observations.len(), 1, "{observations:?}");
    &observations[0]
}
#[test]
fn actual_registry_context_and_refusal_are_retained_before_rollback() {
    let mut store = Store::open_in_memory().unwrap();
    store.load_shapes("one", SHAPES, TS).unwrap();
    put(&mut store, "first").unwrap();
    let (result, observations) = store.observe_validator(|s| put(s, "second"));
    assert!(matches!(result, Err(Error::ValidationFailed { .. })));
    let o = one(&observations);
    let manifest = o.manifest().unwrap();
    manifest.identity().unwrap();
    use sha2::Digest;
    let executable = std::fs::read("/proc/self/exe").unwrap();
    let expected: [u8; 32] = sha2::Sha256::digest(executable).into();
    assert_eq!(manifest.implementation.artifact.0, expected);
    println!(
        "actual incremental execution: coverage={:?}, raw_conforms={:?}, gate_refused={}, checked_first={}, checked_second={}",
        manifest.incremental.coverage,
        o.validator_conforms(),
        o.gate_error().is_some(),
        o.checked_data().unwrap().contains("first"),
        o.checked_data().unwrap().contains("second")
    );
    assert_eq!(
        manifest.loaded_shapes[0],
        NamedContent {
            name: "one".into(),
            version: TS.into(),
            content: SHAPES.as_bytes().to_vec()
        }
    );
    assert_eq!(manifest.incremental.selected_shapes, manifest.loaded_shapes);
    assert_eq!(manifest.incremental.coverage, ContextCoverage::Complete);
    assert_eq!(o.validator_conforms(), Some(false));
    assert!(o.gate_error().is_some());
    let data = o.checked_data().unwrap();
    assert!(data.contains("first") && data.contains("second"));
    assert_eq!(data.lines().collect::<Vec<_>>(), manifest.incremental.facts);
    assert_eq!(manifest.staged_graph, data.as_bytes());
    let feedback = crate::validate_shapes(o.effective_shapes().unwrap(), data).unwrap();
    assert_eq!(feedback.conforms, o.validator_conforms().unwrap());
    assert!(manifest.implementation.compiled_features.contains("shacl"));
    assert_eq!(
        manifest.implementation.dependency_lock,
        snapshot::digest(include_bytes!("../../../Cargo.lock"))
    );
    assert!(o.coordinates().unwrap().transaction > 0);
    assert_eq!(o.coordinates().unwrap().accepted_at, TS);
    assert_eq!(
        store
            .entity_facts(store.lookup("urn:e").unwrap().unwrap())
            .unwrap()
            .len(),
        1
    );
}
#[test]
fn no_shapes_and_explicit_empty_are_distinct_unknown_executions() {
    for explicit in [None, Some("")] {
        let mut store = Store::open_in_memory().unwrap();
        let (result, observations) =
            store.observe_validator(|s| s.with_write_shapes(explicit, |s| put(s, "ok")));
        result.unwrap();
        let o = one(&observations);
        let m = o.manifest().unwrap();
        assert_eq!(
            m.incremental.write_shapes,
            explicit.map(|s| s.as_bytes().to_vec())
        );
        assert_eq!(m.incremental.coverage, ContextCoverage::Unknown);
        assert!(o.checked_data().is_none());
    }
}
#[test]
fn full_new_graph_and_conditional_selection_bind_actual_shapes() {
    for conditional in [false, true] {
        let mut store = Store::open_in_memory().unwrap();
        store.load_shapes("one", SHAPES, TS).unwrap();
        store.shacl_config.validate_on_write = !conditional;
        let d = datum(&store, "urn:e", "urn:p", Value::Str("ok".into()));
        let expected = crate::store::conditional::ExpectedValue {
            entity: d.entity,
            attribute: d.attribute,
            value: None,
        };
        let (result, observations) = store.observe_validator(|s| {
            if conditional {
                s.transact_to_graph_if(&[d], TS, None, None, 0, &[expected])
            } else {
                s.transact(&[d], TS, None, None)
            }
        });
        result.unwrap();
        let m = one(&observations).manifest().unwrap();
        assert_eq!(m.incremental.conditional, conditional);
        assert_eq!(m.incremental.selected_shapes, m.loaded_shapes);
        assert_eq!(m.incremental.coverage, ContextCoverage::Complete);
        assert_eq!(m.implementation.options["context_mode"], "whole-new-graph");
    }
}
#[test]
fn optional_budget_fallback_binds_only_the_truncated_checked_context() {
    // Same actual optimization boundary as write_late_targeting, with no shadow selector.
    let mut store = Store::open_in_memory().unwrap();
    let shapes = "@prefix sh:<http://www.w3.org/ns/shacl#>. <urn:S> a sh:NodeShape; sh:targetClass <urn:C>; sh:property [sh:path <urn:p>; sh:node <urn:Child>; sh:maxCount 1200]. <urn:Child> sh:property [sh:path <urn:name>; sh:minLength 3].";
    let mut datums = Vec::new();
    for i in 0..1200 {
        let child = format!("urn:child:{i}");
        datums.push(datum(
            &store,
            &child,
            "urn:name",
            Value::Str("valid".into()),
        ));
        datums.push(datum(
            &store,
            "urn:e",
            "urn:p",
            Value::Ref(store.intern(&child).unwrap()),
        ));
    }
    store.transact(&datums, TS, None, None).unwrap();
    let typed = datum(
        &store,
        "urn:e",
        "http://www.w3.org/1999/02/22-rdf-syntax-ns#type",
        Value::Ref(store.intern("urn:C").unwrap()),
    );
    let (result, observations) = store.observe_validator(|s| {
        s.with_write_shapes(Some(shapes), |s| s.transact(&[typed], TS, None, None))
    });
    result.unwrap();
    let o = one(&observations);
    let m = o.manifest().unwrap();
    assert_eq!(m.incremental.coverage, ContextCoverage::BudgetExhausted);
    assert!(store.take_poststate_profile().check_value_fallbacks > 0);
    assert!(!o.checked_data().unwrap().contains("<urn:name>"));
    assert!(String::from_utf8_lossy(&m.staged_graph).contains("<urn:name>"));
    assert_eq!(
        m.incremental.facts.len(),
        o.checked_data().unwrap().lines().count()
    );
}
#[test]
fn missing_observation_and_unwind_cannot_leave_a_complete_record() {
    let mut store = Store::open_in_memory().unwrap();
    let (_, observations) = store.observe_validator(|_| ());
    assert!(observations.is_empty());
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        store.observe_validator(|_| panic!("fixture"))
    }));
    assert!(store.validator_observation.borrow().is_none());
}
#[cfg(feature = "owl")]
#[test]
fn ontology_cache_source_tracks_build_not_current_registry_and_staged_inference() {
    let mut store = Store::open_in_memory().unwrap();
    let ontology =
        "@prefix rdfs:<http://www.w3.org/2000/01/rdf-schema#>. <urn:p> rdfs:domain <urn:C>.";
    store.load_ontology("domain", ontology, TS).unwrap();
    store.load_shapes("one", SHAPES, TS).unwrap();
    let (result, observations) = store.observe_validator(|s| put(s, "first"));
    result.unwrap();
    let m = one(&observations).manifest().unwrap();
    assert_eq!(
        m.ontology_cache_source,
        Some(snapshot::digest(ontology.as_bytes()))
    );
    assert!(String::from_utf8_lossy(&m.staged_graph).contains("<urn:C>"));
    let changed = format!("{ontology}\n<urn:q> rdfs:domain <urn:D>.");
    store
        .load_ontology("domain", &changed, "2026-01-02T00:00:00Z")
        .unwrap();
    // The existing &self registry API cannot invalidate a live cache. Observe
    // its real stale source rather than presenting the new registry as fresh.
    let (result, observations) = store.observe_validator(|s| {
        let d = datum(s, "urn:f", "urn:p", Value::Str("second".into()));
        s.transact(&[d], TS, None, None)
    });
    result.unwrap();
    let m = one(&observations).manifest().unwrap();
    assert_eq!(
        m.ontology_cache_source,
        Some(snapshot::digest(ontology.as_bytes()))
    );
    assert_ne!(
        m.ontology_cache_source,
        Some(snapshot::digest(&m.combined_ontologies))
    );
}

#[test]
fn invalid_validator_inputs_remain_unknown_and_preserve_refusal() {
    let mut store = Store::open_in_memory().unwrap();
    let (result, observations) = store
        .observe_validator(|s| s.with_write_shapes(Some("invalid Turtle {"), |s| put(s, "ok")));
    assert!(result.is_err());
    let o = one(&observations);
    assert_eq!(
        o.manifest().unwrap().incremental.coverage,
        ContextCoverage::Unknown
    );
    assert!(o.gate_error().is_some());
    assert!(o.checked_data().is_none());
}

#[cfg(feature = "owl")]
#[test]
fn actual_empty_ontology_lookup_proves_empty_cache_source() {
    let mut store = Store::open_in_memory().unwrap();
    store.load_shapes("one", SHAPES, TS).unwrap();
    assert!(store.owl_cache_source.is_none());
    let (result, observations) = store.observe_validator(|s| put(s, "ok"));
    result.unwrap();
    let m = one(&observations).manifest().unwrap();
    assert!(m.loaded_ontologies.is_empty());
    assert_eq!(m.ontology_cache_source, Some(snapshot::digest(b"")));
    m.identity().unwrap();
}

#[test]
fn debt_revalidation_binds_actual_pre_and_post_inputs() {
    let mut store = Store::open_in_memory().unwrap();
    put(&mut store, "x").unwrap();
    let shapes = "@prefix sh:<http://www.w3.org/ns/shacl#>. <urn:S> a sh:NodeShape; sh:targetSubjectsOf <urn:p>; sh:property [sh:path <urn:p>; sh:minLength 3].";
    store.load_shapes("debt", shapes, TS).unwrap();
    let changed = datum(&store, "urn:e", "urn:q", Value::Str("new".into()));
    let (result, observations) =
        store.observe_validator(|s| s.transact(&[changed], TS, None, None));
    result.unwrap();
    let o = one(&observations);
    assert_eq!(o.validator_conforms(), Some(false));
    assert!(
        o.gate_error().is_none(),
        "pre-existing debt remains admitted"
    );
    let m = o.manifest().unwrap();
    let pre: serde_json::Value =
        serde_json::from_str(&m.implementation.options["debt_pre"]).unwrap();
    let post: serde_json::Value =
        serde_json::from_str(&m.implementation.options["debt_post"]).unwrap();
    assert!(pre["data"].as_str().unwrap().contains("<urn:p> \"x\""));
    assert!(!pre["data"].as_str().unwrap().contains("<urn:q>"));
    assert_eq!(post["data"].as_str().unwrap(), o.checked_data().unwrap());
    assert_eq!(
        pre["shapes"].as_str().unwrap(),
        o.effective_shapes().unwrap()
    );
    assert_eq!(post["shapes"], pre["shapes"]);
    assert_eq!(pre["conforms"], false);
    assert_eq!(post["conforms"], false);
    m.identity().unwrap();
}

#[test]
fn exhausted_outer_context_records_skip_instead_of_conformance() {
    let mut store = Store::open_in_memory().unwrap();
    let entity = store.intern("urn:e").unwrap();
    let attribute = store.intern("urn:p").unwrap();
    let budget = crate::store::shacl_poststate::CONTEXT_FACT_BUDGET;
    let seed: Vec<_> = (0..=budget)
        .map(|i| Datum {
            entity,
            attribute,
            value: Value::Str(format!("v{i}")),
            valid_from: TS.into(),
            valid_to: None,
            op: Op::Assert,
        })
        .collect();
    store.transact(&seed, TS, None, None).unwrap();
    let shapes = "@prefix sh:<http://www.w3.org/ns/shacl#>. <urn:S> a sh:NodeShape; sh:targetSubjectsOf <urn:p>; sh:property [sh:path <urn:p>; sh:minLength 1].";
    let changed = datum(&store, "urn:e", "urn:q", Value::Str("new".into()));
    let (result, observations) = store.observe_validator(|s| {
        s.with_write_shapes(Some(shapes), |s| s.transact(&[changed], TS, None, None))
    });
    result.unwrap();
    let o = one(&observations);
    let m = o.manifest().unwrap();
    assert_eq!(m.incremental.coverage, ContextCoverage::BudgetExhausted);
    assert_eq!(m.incremental.fact_budget, u64::from(budget));
    assert!(m.implementation.options["context_exhaustion"].contains("100000 facts"));
    assert!(o.checked_data().is_none());
    assert_eq!(o.validator_conforms(), None);
    assert!(m.incremental.facts.is_empty());
    assert!(
        o.gate_error().is_none(),
        "existing value-only exhaustion skips validation"
    );
}
