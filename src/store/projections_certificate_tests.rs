//! aegis-eif2sj: a projection certificate counts, in the aggregate's own
//! snapshot, exactly the anomalies a normalizing client would otherwise have
//! to discover by re-reading every subject.

use serde_json::json;

use super::{ProjectionAggregate, ProjectionCertificate};
use crate::Store;
use crate::config::{ProjectionColumnConfig, ProjectionConfig};

const EX: &str = "http://ex.test/";

fn def() -> ProjectionConfig {
    ProjectionConfig {
        name: "items".into(),
        key: format!("{EX}status"),
        columns: vec![ProjectionColumnConfig {
            name: "identifier".into(),
            predicate: format!("{EX}identifier"),
            time: false,
            equals: None,
        }],
    }
}

fn knot(store: &mut Store, ttl: &str) {
    crate::tool_knot(
        store,
        &json!({"turtle": format!("@prefix ex: <{EX}> . {ttl}"),
                "timestamp": "2026-11-11T00:00:00Z"}),
    )
    .unwrap();
}

fn store_with(ttl: &str) -> Store {
    let mut store = Store::open_in_memory().unwrap();
    knot(&mut store, ttl);
    store.install_projections(&[def()]).unwrap();
    store
}

fn certified(store: &Store, unique: &[&str]) -> serde_json::Value {
    store
        .aggregate_projection_certified(
            &def(),
            &ProjectionAggregate {
                graph: "",
                ..Default::default()
            },
            &ProjectionCertificate {
                class: &format!("{EX}Item"),
                unique,
            },
        )
        .unwrap()
}

const CLEAN: &str = r#"ex:a a ex:Item ; ex:status "open" ; ex:identifier "s-1" .
                       ex:b a ex:Item ; ex:status "closed" ; ex:identifier "s-2" ."#;

#[test]
fn a_clean_graph_certifies_with_every_anomaly_zero() {
    let store = store_with(CLEAN);
    let out = certified(&store, &["identifier"]);
    assert_eq!(out["total"], 2, "the aggregate answer is unchanged: {out}");
    let c = &out["certificate"];
    assert_eq!(c["scope"], "graph");
    assert_eq!(c["rows"], 2);
    assert_eq!(c["members"], 2);
    assert_eq!(c["members_without_key"], 0);
    assert_eq!(c["rows_without_class"], 0);
    assert_eq!(c["key_not_plain"], 0);
    assert_eq!(
        c["unique"]["identifier"],
        json!({"missing": 0, "not_plain": 0, "duplicated": 0})
    );
    // One snapshot: the certificate and the aggregate name the same tx.
    assert_eq!(c["tx"], out["tx"]);
}

#[test]
fn a_typed_subject_without_a_key_is_counted() {
    // The board case: a work item whose status fact is missing is invisible
    // to the projection, so its count would silently drop it.
    let store = store_with(&format!("{CLEAN} ex:c a ex:Item ; ex:identifier \"s-3\" ."));
    let c = &certified(&store, &[])["certificate"];
    assert_eq!(c["members"], 3);
    assert_eq!(c["rows"], 2);
    assert_eq!(c["members_without_key"], 1);
    assert_eq!(c["rows_without_class"], 0);
}

#[test]
fn a_keyed_row_outside_the_class_is_counted() {
    let store = store_with(&format!("{CLEAN} ex:x ex:status \"open\" ."));
    let c = &certified(&store, &[])["certificate"];
    assert_eq!(c["rows"], 3);
    assert_eq!(c["members"], 2);
    assert_eq!(c["rows_without_class"], 1);
}

#[test]
fn a_non_plain_key_is_counted() {
    let store = store_with(&format!(
        "{CLEAN} ex:n a ex:Item ; ex:status 7 ; ex:identifier \"s-9\" ."
    ));
    let c = &certified(&store, &[])["certificate"];
    assert_eq!(c["key_not_plain"], 1);
}

#[test]
fn unique_columns_report_missing_non_plain_and_duplicated_values() {
    let store = store_with(&format!(
        "{CLEAN}
         ex:d a ex:Item ; ex:status \"open\" ; ex:identifier \"s-1\" .
         ex:e a ex:Item ; ex:status \"open\" .
         ex:f a ex:Item ; ex:status \"open\" ; ex:identifier 5 ."
    ));
    let c = &certified(&store, &["identifier"])["certificate"];
    // ex:a and ex:d share s-1: both rows are duplicated, not one.
    assert_eq!(
        c["unique"]["identifier"],
        json!({"missing": 1, "not_plain": 1, "duplicated": 2})
    );
}

#[test]
fn an_unknown_class_has_no_members_and_every_row_lacks_it() {
    let store = store_with(CLEAN);
    let out = store
        .aggregate_projection_certified(
            &def(),
            &ProjectionAggregate {
                graph: "",
                ..Default::default()
            },
            &ProjectionCertificate {
                class: &format!("{EX}NeverWritten"),
                unique: &[],
            },
        )
        .unwrap();
    assert_eq!(out["certificate"]["members"], 0);
    assert_eq!(out["certificate"]["rows_without_class"], 2);
}

#[test]
fn a_write_after_install_moves_the_certificate() {
    let mut store = store_with(CLEAN);
    let before = certified(&store, &[]);
    knot(&mut store, "ex:g a ex:Item .");
    let after = certified(&store, &[]);
    assert_eq!(after["certificate"]["members_without_key"], 1);
    assert_ne!(after["certificate"]["tx"], before["certificate"]["tx"]);
}

#[test]
fn malformed_certificate_requests_are_refused() {
    let store = store_with(CLEAN);
    let run = |graphs: &[&str], class: &str, unique: &[&str]| {
        store
            .aggregate_projection_certified(
                &def(),
                &ProjectionAggregate {
                    graph: "",
                    graphs,
                    ..Default::default()
                },
                &ProjectionCertificate { class, unique },
            )
            .unwrap_err()
            .to_string()
    };
    let class = format!("{EX}Item");
    assert!(run(&["urn:g"], &class, &[]).contains("one graph"));
    assert!(run(&[], " ", &[]).contains("class is required"));
    assert!(run(&[], &class, &["nope"]).contains("not declared"));
    assert!(run(&[], &class, &["a", "b", "c", "d", "e"]).contains("at most"));
}
