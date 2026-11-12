use super::*;
use std::collections::BTreeSet;
#[test]
fn aggregate_projection_reifies_result_and_keeps_all_source_events() {
    let state = Current {
        values: BTreeSet::from([Scalar::Integer(7)]),
        produced_by: BTreeSet::from(["urn:delta3".into(), "urn:delta4".into()]),
    };
    let a = project("urn:g", "urn:fluent", "urn:s", "urn:p", &state).unwrap();
    let b = project("urn:g", "urn:fluent", "urn:s", "urn:p", &state).unwrap();
    assert_eq!(a, b);
    assert_eq!(a.statements.len(), 3);
    let Term::Triple(inner) = &a.statements[0].object else {
        panic!("must be a triple term")
    };
    assert_eq!(
        inner.object,
        Term::Literal(Literal::new_typed_literal("7", oxrdf::vocab::xsd::INTEGER))
    );
    assert!(a.statements.iter().all(|t| t.subject != inner.subject));
    assert_eq!(a.statements[1].object, node("urn:delta3"));
    assert_eq!(a.statements[2].object, node("urn:delta4"));
    let other = project("urn:other", "urn:fluent", "urn:s", "urn:p", &state).unwrap();
    assert_ne!(a.statements[0].subject, other.statements[0].subject);
}
fn node(iri: &str) -> Term {
    NamedNode::new(iri.to_owned()).unwrap().into()
}
#[test]
fn set_projection_preserves_terms_and_invalid_values_refuse_atomically() {
    let mut state = Current {
        values: BTreeSet::from([
            Scalar::Term("\"01\"^^<http://www.w3.org/2001/XMLSchema#integer>".into()),
            Scalar::Term("\"01\"".into()),
        ]),
        produced_by: BTreeSet::from(["urn:event".into()]),
    };
    let p = project("urn:g", "urn:f", "urn:s", "urn:p", &state).unwrap();
    assert_eq!(p.statements.len(), 4);
    assert_ne!(p.statements[0].subject, p.statements[2].subject);
    state.values.insert(Scalar::Term("_:unstable".into()));
    assert!(project("urn:g", "urn:f", "urn:s", "urn:p", &state).is_err());
    assert!(project("relative", "urn:f", "urn:s", "urn:p", &state).is_err());
}

#[test]
fn combined_statement_and_input_byte_budgets_refuse_before_returning_projection() {
    let values = (0..17).map(Scalar::Integer).collect();
    let events = (0..256).map(|i| format!("urn:event:{i}")).collect();
    let state = Current {
        values,
        produced_by: events,
    };
    assert!(project("urn:g", "urn:f", "urn:s", "urn:p", &state).is_err());
    let state = Current {
        values: BTreeSet::from([Scalar::Term(format!("\"{}\"", "x".repeat(65537)))]),
        produced_by: BTreeSet::from(["urn:event".into()]),
    };
    assert!(project("urn:g", "urn:f", "urn:s", "urn:p", &state).is_err());
}
