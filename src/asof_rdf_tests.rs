use std::collections::BTreeSet;

use oxrdfio::{RdfFormat, RdfParser};
use serde_json::json;

use super::*;

const GRAPH: &str = "urn:test:board";

fn binding() -> Binding {
    Binding {
        source: Source {
            store_id: "test-store".into(),
            log_epoch: "test-epoch".into(),
        },
        watermark_tx: 19,
        graph: GRAPH.into(),
        shapes_hash: "sha256:profile-at-19".into(),
    }
}

fn limits() -> Limits {
    Limits {
        frame_bytes: 1024 * 1024,
        total_bytes: 1024 * 1024,
        frames: 100,
    }
}

fn records() -> Vec<Json> {
    // Value first, type/late target last. Different physical owners may repeat
    // a statement; RDF union semantics still has exactly one triple.
    [
        ("urn:p:value", json!("bonjour")),
        ("urn:p:language", json!({"value":"bonjour", "lang":"fr"})),
        (
            "urn:p:typed",
            json!({"value":"001", "datatype":"urn:test:datatype"}),
        ),
        ("urn:p:link", json!({"ref":"urn:item:other"})),
        ("urn:p:value", json!("bonjour")),
        (
            "http://www.w3.org/1999/02/22-rdf-syntax-ns#type",
            json!({"ref":"urn:test:Target"}),
        ),
    ]
    .into_iter()
    .map(|(attribute, value)| {
        json!({
            "graph":GRAPH,"entity":"urn:item:subject","attribute":attribute,
            "value":value,"op":"assert","tx":1,"timestamp":"2026-11-04T00:00:00Z",
            "valid_from":"2026-11-04T00:00:00Z","valid_to":null,
        })
    })
    .collect()
}

fn manifest(records: &[Json]) -> Json {
    let held: Vec<_> = records.iter().collect();
    json!({
        "schema":"quipu-asof-share/v1",
        "source":{"store_id":"test-store","log_epoch":"test-epoch"},
        "watermark_tx":19,"source_head_tx":19,"scope":[GRAPH],"context_graphs":[],
        "scope_digest":"sha256:scope","policy_digest":"sha256:policy",
        "shapes_hash":"sha256:profile-at-19",
        "completeness":{"valid_time":true,"bytes":"lossless","graphs":{
            GRAPH:{"records":records.len(),"digest":format!("sha256:{}",crate::applier::share_graph_digest(&held)),"frozen":null}
        }},
        "producer":{"version":"test","git_sha":"test","git_dirty":false}
    })
}

fn frames(records: &[Json]) -> Vec<(String, String)> {
    let mut parts = crate::feed_parts::PartBuilder::with_max_len(19, 600);
    let mut bodies = Vec::new();
    for record in records {
        if let Some(body) = parts.push(record).unwrap() {
            bodies.push(body);
        }
    }
    let (last, summary) = parts.finish();
    if let Some(body) = last {
        bodies.push(body);
    }
    let mut output = vec![(
        "quipu.asof.begin".into(),
        json!({
            "manifest":manifest(records),"parts":summary.parts,"records":summary.records,
            "digest":summary.digest,"source_tx":19,
        })
        .to_string(),
    )];
    output.extend(
        bodies
            .into_iter()
            .map(|data| ("quipu.asof.part".into(), data)),
    );
    output.push((
        "quipu.asof.commit".into(),
        json!({"source_tx":19,"digest":summary.digest}).to_string(),
    ));
    output
}

fn run(frames: Vec<(String, String)>) -> Result<(Receipt, BTreeSet<String>)> {
    let mut store = Store::open_in_memory()?;
    let mut union = BTreeSet::new();
    let receipt = convert(
        &mut store,
        &binding(),
        &limits(),
        frames.into_iter().map(Ok),
        |_, bytes| {
            for quad in RdfParser::from_format(RdfFormat::Turtle).for_reader(bytes) {
                union.insert(quad.map_err(|_| invalid("bad RDF output"))?.to_string());
            }
            Ok(())
        },
    )?;
    Ok((receipt, union))
}

#[test]
fn chunked_and_whole_cut_have_the_same_rdf_union() {
    let records = records();
    let (chunked, union) = run(frames(&records)).unwrap();
    assert!(chunked.chunks > 1);
    assert_eq!(union.len(), 5);
    let (whole, whole_union) = run(vec![(
        "quipu.asof".into(),
        json!({
            "manifest":manifest(&records),"records":records,
        })
        .to_string(),
    )])
    .unwrap();
    assert_eq!(whole.chunks, 1);
    assert_eq!(union, whole_union);
    assert!(union.iter().any(|row| row.contains("@fr")));
    assert!(
        union
            .iter()
            .any(|row| row.contains("\"001\"^^<urn:test:datatype>"))
    );
}

#[test]
fn incomplete_reordered_mixed_and_extra_cuts_are_refused() {
    let original = frames(&records());
    let mut missing = original.clone();
    missing.pop();
    let mut reordered = original.clone();
    reordered.swap(1, 2);
    let mut mixed = original.clone();
    let mut part: Json = serde_json::from_str(&mixed[1].1).unwrap();
    part["source_tx"] = json!(20);
    mixed[1].1 = part.to_string();
    let mut extra = original.clone();
    extra.extend(original.clone());
    let mut wrong_scope = original.clone();
    let mut begin: Json = serde_json::from_str(&wrong_scope[0].1).unwrap();
    begin["manifest"]["scope"] = json!(["urn:other"]);
    wrong_scope[0].1 = begin.to_string();
    for bad in [missing, reordered, mixed, extra, wrong_scope] {
        assert!(run(bad).is_err());
    }
}

#[test]
fn profile_epoch_and_all_budgets_refuse_without_acceptance() {
    for (key, value) in [
        ("shapes_hash", json!("sha256:new")),
        ("watermark_tx", json!(20)),
        (
            "source",
            json!({"store_id":"test-store","log_epoch":"other"}),
        ),
    ] {
        let mut cut = frames(&records());
        let mut begin: Json = serde_json::from_str(&cut[0].1).unwrap();
        begin["manifest"][key] = value;
        cut[0].1 = begin.to_string();
        assert!(run(cut).is_err());
    }
    for limits in [
        Limits {
            frame_bytes: 1,
            ..limits()
        },
        Limits {
            total_bytes: 1,
            ..limits()
        },
        Limits {
            frames: 1,
            ..limits()
        },
    ] {
        let mut store = Store::open_in_memory().unwrap();
        assert!(
            convert(
                &mut store,
                &binding(),
                &limits,
                frames(&records()).into_iter().map(Ok),
                |_, _| Ok(())
            )
            .is_err()
        );
        assert!(store.current_facts().unwrap().is_empty());
        assert!(store.conn.is_autocommit());
    }
}

#[test]
fn iterator_and_output_failure_roll_back_the_open_share() {
    for sink_failure in [false, true] {
        let mut store = Store::open_in_memory().unwrap();
        let mut input: Vec<_> = frames(&records()).into_iter().map(Ok).collect();
        if !sink_failure {
            input[2] = Err(invalid("read failure"));
        }
        assert!(
            convert(&mut store, &binding(), &limits(), input, |_, _| {
                if sink_failure {
                    Err(invalid("disk failure"))
                } else {
                    Ok(())
                }
            })
            .is_err()
        );
        assert!(store.conn.is_autocommit());
        let count: i64 = store
            .conn
            .query_row("SELECT COUNT(*) FROM facts", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }
}
