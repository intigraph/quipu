use super::*;

fn seq(values: &[&str]) -> Option<Vec<String>> {
    Some(values.iter().map(|s| format!("\"{s}\"")).collect())
}
fn slot() -> Slot {
    ("<urn:owner>".into(), "urn:list".into())
}

#[test]
fn c6_whole_and_sequence_oracles() {
    let b = seq(&["x", "y"]);
    assert_eq!(
        merge(&b, &b, &seq(&["x", "y", "z"]), Mode::Whole).unwrap(),
        Some(seq(&["x", "y", "z"]))
    );
    assert_eq!(
        merge(&b, &seq(&["y", "x"]), &seq(&["x", "y", "z"]), Mode::Whole).unwrap(),
        None
    );
    let agreed = seq(&["x", "y", "z"]);
    assert_eq!(
        merge(&b, &agreed, &agreed, Mode::Whole).unwrap(),
        Some(agreed)
    );
    assert_eq!(
        merge(
            &b,
            &seq(&["w", "x", "y"]),
            &seq(&["x", "y", "z"]),
            Mode::Sequence
        )
        .unwrap(),
        Some(seq(&["w", "x", "y", "z"]))
    );
    assert_eq!(
        merge(&b, &seq(&["x", "q"]), &seq(&["x", "r"]), Mode::Sequence).unwrap(),
        None
    );
    assert_ne!(
        identity(&None).unwrap(),
        identity(&Some(Vec::new())).unwrap()
    );
}

#[test]
fn cells_are_not_identity_and_lowering_is_repeatable() {
    let value = seq(&["x", "x", "y"]);
    let graph = lower(&slot(), &value).unwrap();
    let slots = BTreeMap::from([(slot(), Mode::Whole)]);
    assert_eq!(
        read(&graph, &slots).unwrap()[&slot()]
            .as_ref()
            .unwrap()
            .elements,
        value.clone().unwrap()
    );
    let rendered: String = graph.iter().map(|t| format!("{t} .\n")).collect();
    let relabelled = rendered.replace("urn:quipu:list:", "urn:other:");
    let other = read(&parse_graph(&relabelled, "renamed").unwrap(), &slots).unwrap();
    assert_eq!(
        other[&slot()].as_ref().unwrap().elements,
        value.clone().unwrap()
    );
    assert_eq!(lower(&slot(), &value).unwrap(), graph);
}

#[test]
fn malformed_cycles_orphans_and_budget_overflow_refuse() {
    let s = slot();
    let slots = BTreeMap::from([(s.clone(), Mode::Whole)]);
    let nil = format!("<{RDF}nil>");
    for body in [
        format!(
            "{} <{}> _:a .\n_:a <{RDF}first> \"x\" .\n_:a <{RDF}rest> _:a .\n",
            s.0, s.1
        ),
        format!("{} <{}> _:a .\n_:a <{RDF}first> \"x\" .\n", s.0, s.1),
        format!(
            "{} <{}> {nil} .\n_:orphan <{RDF}first> \"x\" .\n_:orphan <{RDF}rest> {nil} .\n",
            s.0, s.1
        ),
    ] {
        assert!(read(&parse_graph(&body, "bad").unwrap(), &slots).is_err());
    }
    let large = Some(vec!["\"x\"".into(); MAX_CELLS]);
    let changed = Some(vec!["\"y\"".into(); MAX_CELLS]);
    assert!(
        merge(
            &large,
            &changed,
            &Some(vec!["\"z\"".into()]),
            Mode::Sequence
        )
        .is_err()
    );
}

#[test]
fn lowering_and_noop_planning_refuse_unidentified_blank_elements() {
    // Parsing trims whitespace: checking the raw prefix alone would fail open.
    for element in ["_:nested", "  _:nested  "] {
        let value = Some(vec![element.to_owned()]);
        assert!(lower(&slot(), &value).is_err());
        assert!(
            plan(
                &slot(),
                &value,
                &value,
                &value,
                Mode::Whole,
                None,
                "2026-01-01",
                "2026-01-02",
                "2026-01-03",
            )
            .is_err()
        );
    }
    // A literal spelling a blank-node label has ordinary literal identity.
    let literal = seq(&["_:nested"]);
    let graph = lower(&slot(), &literal).unwrap();
    let slots = BTreeMap::from([(slot(), Mode::Whole)]);
    assert_eq!(
        read(&graph, &slots).unwrap()[&slot()]
            .as_ref()
            .unwrap()
            .elements,
        literal.unwrap()
    );
}
