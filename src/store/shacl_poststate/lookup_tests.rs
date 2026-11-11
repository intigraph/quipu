//! Actual-helper identity and work regressions for bound incoming targets.
use super::*;
use crate::{Datum, Fact, Op, Value};

const TS: &str = "2026-11-09T00:00:00Z";
const P: &str = "https://example.org/incoming";
const Q: &str = "https://example.org/other";

fn write(s: &mut Store, name: &str, p: &str, v: Value, g: i64, op: Op) -> i64 {
    let d = Datum {
        entity: s.intern(name).unwrap(),
        attribute: s.intern(p).unwrap(),
        value: v,
        valid_from: TS.into(),
        valid_to: None,
        op,
    };
    s.transact_to_graph(&[d], TS, None, None, g).unwrap()
}

fn key(f: &Fact) -> (i64, i64, Vec<u8>, i64, String, Option<String>) {
    (
        f.entity,
        f.attribute,
        f.value.to_bytes(),
        f.tx,
        f.valid_from.clone(),
        f.valid_to.clone(),
    )
}

#[test]
fn incoming_lookup_preserves_complete_row_identity_across_values_graphs_and_history() {
    let mut s = Store::open_in_memory().unwrap();
    let g = s.graph_create("https://example.org/graph").unwrap();
    let other = s.graph_create("https://example.org/other-graph").unwrap();
    let values = [
        Value::Ref(s.intern("https://example.org/target").unwrap()),
        Value::Str("word".into()),
        Value::Int(7),
        Value::Typed {
            lexical: "2026-11-09".into(),
            datatype: "http://www.w3.org/2001/XMLSchema#date".into(),
        },
        Value::Lang {
            lexical: "word".into(),
            lang: "en".into(),
        },
    ];
    for (i, value) in values.into_iter().enumerate() {
        for graph in [0, g] {
            let name = format!("https://example.org/keep-{i}-{graph}");
            let tx = write(&mut s, &name, P, value.clone(), graph, Op::Assert);
            let second = format!("https://example.org/second-{i}-{graph}");
            let tx2 = write(&mut s, &second, Q, value.clone(), graph, Op::Assert);
            write(
                &mut s,
                &format!("https://example.org/excluded-{i}-{graph}"),
                P,
                value.clone(),
                other,
                Op::Assert,
            );
            let gone = format!("https://example.org/gone-{i}-{graph}");
            write(&mut s, &gone, P, value.clone(), graph, Op::Assert);
            write(&mut s, &gone, P, value.clone(), graph, Op::Retract);
            let predicates =
                BTreeSet::from([P.into(), Q.into(), "https://example.org/missing".into()]);
            let mut actual = Vec::new();
            s.append_object_targets(&mut actual, &value, graph, &predicates)
                .unwrap();
            actual.sort_by_key(key);
            let mut old = Vec::new();
            for p in [P, Q] {
                let mut stmt = s.conn.prepare("SELECT e,a,v,tx,valid_from,valid_to,op FROM facts INDEXED BY idx_active_vge WHERE v=?1 AND a=?2 AND g=?3 AND op=1 AND valid_to IS NULL LIMIT 100001").unwrap();
                let mut rows = stmt
                    .query(rusqlite::params![
                        value.to_bytes(),
                        s.lookup(p).unwrap().unwrap(),
                        graph
                    ])
                    .unwrap();
                while let Some(row) = rows.next().unwrap() {
                    assert_eq!(row.get::<_, i64>(6).unwrap(), 1);
                    old.push(Fact {
                        entity: row.get(0).unwrap(),
                        attribute: row.get(1).unwrap(),
                        value: Value::from_bytes(&row.get::<_, Vec<u8>>(2).unwrap()).unwrap(),
                        tx: row.get(3).unwrap(),
                        valid_from: row.get(4).unwrap(),
                        valid_to: row.get(5).unwrap(),
                        op: Op::Assert,
                    });
                }
            }
            old.sort_by_key(key);
            assert_eq!(
                actual, old,
                "complete identity against old value+graph plan"
            );
            let mut expected = vec![
                Fact {
                    entity: s.lookup(&name).unwrap().unwrap(),
                    attribute: s.lookup(P).unwrap().unwrap(),
                    value: value.clone(),
                    tx,
                    valid_from: TS.into(),
                    valid_to: None,
                    op: Op::Assert,
                },
                Fact {
                    entity: s.lookup(&second).unwrap().unwrap(),
                    attribute: s.lookup(Q).unwrap().unwrap(),
                    value: value.clone(),
                    tx: tx2,
                    valid_from: TS.into(),
                    valid_to: None,
                    op: Op::Assert,
                },
            ];
            expected.sort_by_key(key);
            assert_eq!(actual, expected, "value={value:?}, graph={graph}");
        }
    }
}

#[test]
fn incoming_lookup_bounds_work_for_unrelated_attribute_cohort_and_keeps_cap() {
    let mut s = Store::open_in_memory().unwrap();
    let value = Value::Ref(s.intern("https://example.org/target").unwrap());
    let tx = write(
        &mut s,
        "https://example.org/keep",
        P,
        value.clone(),
        0,
        Op::Assert,
    );
    let unrelated = s.intern(Q).unwrap();
    // One value+graph cohort, mostly a different attribute. Use direct fixture
    // rows to avoid timing thousands of unrelated write transactions.
    s.conn.execute("WITH RECURSIVE n(x) AS (VALUES(1000000) UNION ALL SELECT x+1 FROM n WHERE x<1009999) INSERT INTO facts(e,a,v,g,tx,valid_from,op) SELECT x,?1,?2,0,?3,?4,1 FROM n",
        rusqlite::params![unrelated, value.to_bytes(), tx, TS]).unwrap();
    let predicates = BTreeSet::from([P.into()]);
    let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counter = calls.clone();
    s.conn
        .progress_handler(
            1000,
            Some(move || counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed) >= 10),
        )
        .unwrap();
    let mut actual = Vec::new();
    let result = s.append_object_targets(&mut actual, &value, 0, &predicates);
    s.conn.progress_handler(0, None::<fn() -> bool>).unwrap();
    result.unwrap();
    assert_eq!(actual.len(), 1);
    assert_eq!(
        actual[0].entity,
        s.lookup("https://example.org/keep").unwrap().unwrap()
    );
    assert!(calls.load(std::sync::atomic::Ordering::Relaxed) < 10);
    let existing = actual[0].clone();
    let mut full = vec![existing.clone(); 100_000];
    assert!(matches!(
        s.append_object_targets(&mut full, &value, 0, &predicates),
        Err(Error::CannotVerify(_))
    ));
    assert_eq!(full.len(), 100_000);
    let mut almost = vec![existing; 99_999];
    s.append_object_targets(&mut almost, &value, 0, &predicates)
        .unwrap();
    assert_eq!(almost.len(), 100_000);
}
