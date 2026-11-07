use super::retraction_datums;
use crate::{Fact, Op, Value, namespace};

fn fact(entity: i64, attribute: i64, value: Value, tx: i64) -> Fact {
    Fact {
        entity,
        attribute,
        value,
        tx,
        valid_from: format!("source-time-{tx}"),
        valid_to: Some("old-close".into()),
        op: Op::Assert,
    }
}

#[test]
fn aliases_keep_first_physical_representation_and_input_order() {
    let typed = Value::Typed {
        lexical: "1".into(),
        datatype: namespace::XSD_INTEGER.into(),
    };
    let facts = [
        fact(2, 9, typed.clone(), 10),
        fact(1, 9, Value::Int(1), 11),
        fact(2, 8, Value::Int(1), 12),
        fact(2, 9, Value::Int(1), 13),
        fact(
            2,
            9,
            Value::Typed {
                lexical: "01".into(),
                datatype: namespace::XSD_INTEGER.into(),
            },
            14,
        ),
    ];
    let result = retraction_datums(&facts);
    assert_eq!(result.len(), 4);
    assert_eq!(
        result
            .iter()
            .map(|d| (d.entity, d.attribute))
            .collect::<Vec<_>>(),
        [(2, 9), (1, 9), (2, 8), (2, 9)]
    );
    assert_eq!(result[0].value.to_bytes(), typed.to_bytes());
    assert_eq!(result[0].valid_from, "source-time-10");
    assert_eq!(result[3].valid_from, "source-time-14");
    assert!(
        result
            .iter()
            .all(|d| d.op == Op::Retract && d.valid_to.is_none())
    );
}

#[test]
fn float_term_identity_keeps_signed_zero_and_collapses_nan_aliases() {
    let nan = Value::Float(f64::from_bits(0x7ff8_0000_0000_0001));
    let facts = [
        fact(1, 9, Value::Float(-0.0), 1),
        fact(1, 9, Value::Float(0.0), 2),
        fact(1, 9, nan.clone(), 3),
        fact(1, 9, Value::Float(f64::NAN), 4),
        fact(
            1,
            9,
            Value::Typed {
                lexical: "NaN".into(),
                datatype: namespace::XSD_DOUBLE.into(),
            },
            5,
        ),
    ];
    let result = retraction_datums(&facts);
    assert_eq!(result.len(), 3);
    assert_eq!(result[0].value.to_bytes(), Value::Float(-0.0).to_bytes());
    assert_eq!(result[1].value.to_bytes(), Value::Float(0.0).to_bytes());
    assert_eq!(result[2].value.to_bytes(), nan.to_bytes());
    assert_eq!(result[2].valid_from, "source-time-3");
}

#[test]
fn lossy_codec_key_is_only_an_index_not_a_replacement_for_value_equality() {
    let common = "x".repeat(u16::MAX as usize);
    let first = Value::Typed {
        lexical: "value".into(),
        datatype: format!("{common}a"),
    };
    let second = Value::Typed {
        lexical: "value".into(),
        datatype: format!("{common}b"),
    };
    assert_ne!(first, second);
    assert_eq!(
        first.term_key(),
        second.term_key(),
        "positive key-collision fixture"
    );
    let result = retraction_datums(&[
        fact(1, 9, first.clone(), 1),
        fact(1, 9, second.clone(), 2),
        fact(1, 9, first, 3),
    ]);
    assert_eq!(result.len(), 2);
    assert_eq!(result[1].value, second);
    assert_eq!(result[1].valid_from, "source-time-2");
}

#[test]
fn source_wide_distinct_slots_and_repeated_audit_rows_reconcile() {
    let mut facts = Vec::new();
    for entity in 0..4096 {
        for tx in [1, 2] {
            facts.push(fact(
                entity,
                9,
                Value::Str("same text, separate subject".into()),
                tx,
            ));
        }
    }
    let result = retraction_datums(&facts);
    assert_eq!(result.len(), 4096);
    assert!(
        result
            .iter()
            .enumerate()
            .all(|(n, d)| d.entity == n as i64 && d.valid_from == "source-time-1")
    );
}
