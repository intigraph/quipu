//! Atomic single-predicate replacement.

use super::{Datum, Store, ops::retraction_datums};
use crate::{
    error::{Error, Result},
    types::{Fact, Op, Value},
};

// aegis-byn4fn: the eighth argument is the transaction source. It is the
// handle by which these facts can ever be retracted, so it belongs on the
// write path itself; bundling the parameters into a struct to satisfy the
// lint would hide it from every call site, which is how it came to be
// missing in the first place.
#[allow(clippy::too_many_arguments)]
pub(super) fn set_triple(
    store: &mut Store,
    entity: i64,
    predicate: i64,
    value: Value,
    timestamp: &str,
    actor: Option<&str>,
    explicit_str: bool,
    source: Option<&str>,
) -> Result<(i64, usize, usize)> {
    let current: Vec<Fact> = store
        .entity_facts(entity)?
        .into_iter()
        .filter(|f| f.attribute == predicate)
        .collect();

    if let Value::Str(s) = &value {
        let holds_ref = current.iter().any(|f| matches!(f.value, Value::Ref(_)));
        let holds_str = current.iter().any(|f| matches!(f.value, Value::Str(_)));
        let looks_like_iri = s.contains("://") && !s.chars().any(char::is_whitespace);
        if !explicit_str && ((holds_ref && !holds_str) || (current.is_empty() && looks_like_iri)) {
            let pred_iri = store.resolve(predicate)?;
            return Err(Error::InvalidValue(format!(
                "set refused: object \"{s}\" is a string literal, but <{pred_iri}> \
                 takes an IRI reference. Pass the object as {{\"iri\": \"{s}\"}} to \
                 set an edge, or as {{\"str\": \"{s}\"}} to state that a literal is \
                 intended — a bare IRI-shaped string here is almost always a mis-shaped \
                 edge that no graph traversal can follow."
            )));
        }
    }

    let mut values = current
        .iter()
        .map(|f| f.value.to_bytes())
        .collect::<Vec<_>>();
    values.sort();
    values.dedup();
    let expected = super::conditional::SetExpectation {
        entity,
        attribute: predicate,
        values,
    };
    let already_present = current.iter().any(|f| f.value == value);
    let to_retract: Vec<Fact> = current.into_iter().filter(|f| f.value != value).collect();
    if to_retract.is_empty() && already_present {
        return Ok((0, 0, 0));
    }

    let mut datums = retraction_datums(&to_retract);
    let retracted = datums.len();
    let asserted = usize::from(!already_present);
    {
        datums.push(Datum {
            entity,
            attribute: predicate,
            value,
            valid_from: timestamp.to_string(),
            valid_to: None,
            op: Op::Assert,
        });
    }
    // aegis-byn4fn: the source was the CONSTANT "set" — one key shared by every
    // correction the fleet has ever made (26,008 transactions when measured), so
    // a correction named no author and the retraction handle meant "every /set
    // ever run". The caller's key wins; the fallback is per-actor.
    let tag = crate::store::source_tag::resolve("set", actor, source);
    let tx_id = store.transact_to_graph_scoped_checked(
        &datums,
        timestamp,
        actor,
        Some(&tag),
        0,
        None,
        &[],
        Some(&expected),
    )?;
    Ok((tx_id, retracted, asserted))
}

#[cfg(all(test, feature = "shacl"))]
mod tests {
    use super::*;

    #[test]
    fn set_validates_existing_subject_datatype_and_preserves_refused_slot() {
        let mut store = Store::open_in_memory().unwrap();
        let e = store.intern("http://ex/module").unwrap();
        let p = store.intern("http://ex/path").unwrap();
        let t = store
            .intern("http://www.w3.org/1999/02/22-rdf-syntax-ns#type")
            .unwrap();
        let class = store.intern("http://ex/Module").unwrap();
        let wrong = store.intern("http://ex/not-a-string").unwrap();
        let stamp = "2026-01-01T00:00:00Z";
        let datum = |attribute, value| Datum {
            entity: e,
            attribute,
            value,
            valid_from: stamp.into(),
            valid_to: None,
            op: Op::Assert,
        };
        store
            .transact(
                &[
                    datum(t, Value::Ref(class)),
                    datum(p, Value::Str("old".into())),
                ],
                stamp,
                None,
                None,
            )
            .unwrap();
        store.load_shapes("set-test", r#"
            @prefix sh: <http://www.w3.org/ns/shacl#> .
            @prefix xsd: <http://www.w3.org/2001/XMLSchema#> .
            <http://ex/Shape> a sh:NodeShape; sh:targetClass <http://ex/Module>;
                sh:property [ sh:path <http://ex/path>; sh:datatype xsd:string; sh:minCount 1; sh:maxCount 1 ] .
        "#, stamp).unwrap();
        store.shacl_config.validate_on_write = true;
        let error = store
            .set_triple(e, p, Value::Ref(wrong), stamp, None, false, None)
            .unwrap_err();
        assert!(matches!(error, Error::ValidationFailed { .. }), "{error}");
        let values = store
            .entity_facts(e)
            .unwrap()
            .into_iter()
            .filter(|f| f.attribute == p)
            .map(|f| f.value)
            .collect::<Vec<_>>();
        assert_eq!(values, vec![Value::Str("old".into())]);
        let (_, retracted, asserted) = store
            .set_triple(e, p, Value::Str("new".into()), stamp, None, true, None)
            .unwrap();
        assert_eq!((retracted, asserted), (1, 1));
        assert_eq!(
            store
                .set_triple(e, p, Value::Str("new".into()), stamp, None, true, None)
                .unwrap(),
            (0, 0, 0)
        );
        // Explicit per-operation shapes remain strict even with persistent
        // validation OFF, and unrelated required properties use store context.
        store.shacl_config.validate_on_write = false;
        let shapes = store.get_combined_shapes().unwrap().unwrap();
        assert!(
            store
                .with_write_shapes(Some(&shapes), |store| {
                    store.set_triple(e, p, Value::Ref(wrong), stamp, None, false, None)
                })
                .is_err()
        );
        assert!(
            store
                .set_triple(e, p, Value::Ref(wrong), stamp, None, false, None)
                .is_ok()
        );
    }
    #[test]
    fn set_complete_slot_conflict_is_atomic_and_multivalue_repair_keeps_chosen_fact() {
        let mut store = Store::open_in_memory().unwrap();
        let e = store.intern("http://ex/repair").unwrap();
        let p = store.intern("http://ex/value").unwrap();
        let stamp = "2026-01-01T00:00:00Z";
        let datum = |value| Datum {
            entity: e,
            attribute: p,
            value,
            valid_from: stamp.into(),
            valid_to: None,
            op: Op::Assert,
        };
        let chosen = Value::Str("chosen".into());
        let original = store
            .transact(
                &[datum(chosen.clone()), datum(Value::Str("extra".into()))],
                stamp,
                None,
                None,
            )
            .unwrap();
        let stale = super::super::conditional::SetExpectation {
            entity: e,
            attribute: p,
            values: vec![chosen.to_bytes()],
        };
        let error = store
            .transact_to_graph_scoped_checked(
                &[datum(Value::Str("lost-update".into()))],
                stamp,
                None,
                None,
                0,
                None,
                &[],
                Some(&stale),
            )
            .unwrap_err();
        assert!(matches!(error, Error::ConditionalWriteConflict));
        assert_eq!(store.entity_facts(e).unwrap().len(), 2);
        let (_, retracted, asserted) = store
            .set_triple(e, p, chosen.clone(), stamp, None, true, None)
            .unwrap();
        assert_eq!((retracted, asserted), (1, 0));
        let facts = store.entity_facts(e).unwrap();
        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].value, chosen);
        assert_eq!(
            facts[0].tx, original,
            "duplicate admission assertion must retain chosen history"
        );
    }
}
