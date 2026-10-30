//! `Store::tx_encoded_bound` is an upper bound on what `changes_in_graphs`
//! builds, for the inputs that make records large: long and escaped strings,
//! Ref targets, repeated transaction metadata and retracts.
use super::*;
use crate::store::{Datum, changes::Capture};
use crate::types::{Op, Value};

const T: &str = "2026-10-25T00:00:00Z";

fn datum(store: &Store, entity: &str, value: Value, op: Op) -> Datum {
    Datum {
        entity: store.intern(entity).unwrap(),
        attribute: store.intern("urn:p:title").unwrap(),
        value,
        valid_from: T.into(),
        valid_to: None,
        op,
    }
}

/// The bound and the real encoded size of `tx`'s records in `g`.
fn bound_and_actual(store: &Store, tx: i64, g: i64) -> (u64, u64) {
    let bound = store.tx_encoded_bound(tx, &[g], u64::MAX).unwrap();
    let page = store
        .changes_in_graphs(tx - 1, 1, Capture::OldAndNewValues, &[g])
        .unwrap();
    let actual = page
        .records
        .iter()
        .map(|r| serde_json::to_string(r).unwrap().len() as u64)
        .sum();
    (bound, actual)
}

/// Commit `datums` as one transaction and check the bound covers it.
fn covers(store: &mut Store, datums: &[Datum], actor: Option<&str>, source: Option<&str>) {
    let g = store.graph_create("urn:graph:b").unwrap();
    let tx = store
        .transact_to_graph(datums, T, actor, source, g)
        .unwrap();
    let (bound, actual) = bound_and_actual(store, tx, g);
    assert!(bound >= actual, "bound {bound} < actual {actual}");
}

#[test]
fn the_bound_covers_strings_at_their_worst_escaping() {
    let mut store = Store::open_in_memory().unwrap();
    // Control characters escape to six bytes each.
    let controls: String = (0u8..32).map(char::from).collect::<String>().repeat(50);
    let lang = Value::Lang {
        lexical: controls.clone(),
        lang: "en".into(),
    };
    let datums = [
        datum(&store, "urn:e:1", Value::Str(controls.clone()), Op::Assert),
        datum(&store, "urn:e:2", lang, Op::Assert),
    ];
    covers(&mut store, &datums, None, None);
    // A retract carries its old value.
    let retract = [datum(&store, "urn:e:1", Value::Str(controls), Op::Retract)];
    covers(&mut store, &retract, None, None);
}

#[test]
fn the_bound_counts_a_ref_by_its_target_iri_not_its_blob() {
    let mut store = Store::open_in_memory().unwrap();
    let target = format!("urn:target:{}", "\"\\".repeat(2_000));
    let target_id = store.intern(&target).unwrap();
    let datums = [datum(&store, "urn:e:1", Value::Ref(target_id), Op::Assert)];
    covers(&mut store, &datums, None, None);
}

#[test]
fn the_bound_covers_bytes_as_base64() {
    let mut store = Store::open_in_memory().unwrap();
    let datums = [datum(
        &store,
        "urn:e:1",
        Value::Bytes(vec![0xff; 3_000]),
        Op::Assert,
    )];
    covers(&mut store, &datums, None, None);
}

#[test]
fn the_bound_counts_a_close_absent_from_the_retraction_audit_row() {
    let mut store = Store::open_in_memory().unwrap();
    let g = store.graph_create("urn:graph:close").unwrap();
    let mut d = datum(&store, "urn:example:item", Value::Int(1), Op::Assert);
    store
        .transact_to_graph(&[d.clone()], T, None, None, g)
        .unwrap();
    d.op = Op::Retract;
    let tx = store.transact_to_graph(&[d], T, None, None, g).unwrap();
    // Like a promoted correction: only the assertion carries the retimed
    // close. A long fractional timestamp makes an omitted bound observable.
    let close = format!("2026-03-01T00:00:00.{}Z", "1".repeat(16_000));
    store
        .conn
        .execute(
            "UPDATE facts SET valid_to=?1 WHERE op=1 AND retracted_tx=?2",
            params![close, tx],
        )
        .unwrap();
    let (bound, actual) = bound_and_actual(&store, tx, g);
    assert!(actual > 16_000);
    assert!(bound >= actual, "bound {bound} < actual {actual}");
}

#[test]
fn the_bound_counts_the_metadata_every_record_repeats() {
    let mut store = Store::open_in_memory().unwrap();
    let datums: Vec<Datum> = (0..20)
        .map(|i| datum(&store, &format!("urn:e:{i}"), Value::Int(i), Op::Assert))
        .collect();
    let actor = "a".repeat(5_000);
    let source = "s".repeat(1_000);
    covers(&mut store, &datums, Some(&actor), Some(&source));
}

#[test]
fn counting_stops_once_the_bound_passes_the_cap() {
    let mut store = Store::open_in_memory().unwrap();
    let g = store.graph_create("urn:graph:b").unwrap();
    let datums: Vec<Datum> = (0..100)
        .map(|i| {
            datum(
                &store,
                &format!("urn:e:{i}"),
                Value::Str("v".into()),
                Op::Assert,
            )
        })
        .collect();
    let tx = store.transact_to_graph(&datums, T, None, None, g).unwrap();
    let full = store.tx_encoded_bound(tx, &[g], u64::MAX).unwrap();
    let capped = store.tx_encoded_bound(tx, &[g], 1).unwrap();
    assert!(capped > 1 && capped < full, "capped {capped}, full {full}");
}

#[test]
fn the_bound_counts_strings_past_an_embedded_nul() {
    // SQLite's length() stops at the first NUL of a text value; the bound
    // must count every byte of IRIs and transaction metadata.
    let text = format!("urn:prefix:{}", "\0".repeat(1000));
    let mut store = Store::open_in_memory().unwrap();
    let datums = [datum(&store, &text, Value::Str("v".into()), Op::Assert)];
    covers(&mut store, &datums, None, None);
    let mut store = Store::open_in_memory().unwrap();
    let datums = [datum(
        &store,
        "urn:short",
        Value::Str("v".into()),
        Op::Assert,
    )];
    covers(&mut store, &datums, Some(&text), Some(&text));
}
