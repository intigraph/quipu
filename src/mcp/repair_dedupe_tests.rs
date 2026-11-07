use super::dedupe;
use crate::{Datum, Op, Value, namespace};

fn datum(entity: i64, value: Value, time: &str) -> Datum {
    Datum {
        entity,
        attribute: 9,
        value,
        valid_from: time.into(),
        valid_to: None,
        op: Op::Retract,
    }
}

#[test]
fn canonical_aliases_preserve_first_datum_and_order() {
    let typed = Value::Typed {
        lexical: "1".into(),
        datatype: namespace::XSD_INTEGER.into(),
    };
    let out = dedupe(vec![
        datum(2, typed.clone(), "first"),
        datum(1, Value::Int(1), "other"),
        datum(2, Value::Int(1), "duplicate"),
    ]);
    assert_eq!(out.len(), 2);
    assert_eq!(out[0].entity, 2);
    assert_eq!(out[0].value.to_bytes(), typed.to_bytes());
    assert_eq!(out[0].valid_from, "first");
    assert_eq!(out[1].entity, 1);
}

#[test]
fn a_codec_key_collision_does_not_merge_unequal_values() {
    let prefix = "x".repeat(u16::MAX as usize);
    let a = Value::Typed {
        lexical: "value".into(),
        datatype: format!("{prefix}a"),
    };
    let b = Value::Typed {
        lexical: "value".into(),
        datatype: format!("{prefix}b"),
    };
    assert_ne!(a, b);
    assert_eq!(a.term_key(), b.term_key(), "positive collision control");
    let out = dedupe(vec![
        datum(1, a.clone(), "a"),
        datum(1, b, "b"),
        datum(1, a, "repeat"),
    ]);
    assert_eq!(out.len(), 2);
    assert_eq!(out[1].valid_from, "b");
}

#[test]
fn source_scale_slots_retain_one_first_datum_each() {
    let mut plan = Vec::new();
    for entity in 0..42_789 {
        plan.push(datum(entity, Value::Str("same".into()), "first"));
        plan.push(datum(entity, Value::Str("same".into()), "repeat"));
    }
    let out = dedupe(plan);
    assert_eq!(out.len(), 42_789);
    assert!(
        out.iter()
            .enumerate()
            .all(|(n, d)| { usize::try_from(d.entity) == Ok(n) && d.valid_from == "first" })
    );
}
