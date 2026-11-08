//! Compare the pendant projection with the frozen value-set operator.
use super::*;

fn graph(mask: u8) -> Graph {
    let text: String = (0..3)
        .filter(|bit| mask & (1 << bit) != 0)
        .map(|bit| format!("<urn:test:s> <urn:test:p> \"{bit}\" .\n"))
        .collect();
    parse_graph(&text, "fixture").unwrap()
}

#[test]
fn pendant_core_matches_frozen_operator_for_all_small_value_sets() {
    // All combinations include empty, identical, delete/replace, disjoint
    // additions, set removals, and over-bound values already in a parent.
    for bound in [None, Some(1), Some(2)] {
        let shapes = bound.map_or_else(String::new, |n| {
            format!(
                "@prefix sh: <http://www.w3.org/ns/shacl#> . \
                 [] sh:path <urn:test:p> ; sh:maxCount {n} ."
            )
        });
        for bm in 0..8 {
            for om in 0..8 {
                for tm in 0..8 {
                    let (b, o, t) = (graph(bm), graph(om), graph(tm));
                    let expected = crate::share_merge::merge_graphs(&b, &o, &t, &shapes).unwrap();
                    let actual = merge(&b, &o, &t, &shapes).unwrap();
                    assert_eq!(actual, expected, "bound={bound:?}, masks={bm}/{om}/{tm}");
                    // Compare the actual canonical pendant payload and decisions
                    // bytes, not just RDF set equality (which ignores order).
                    let bytes = |result: &(Graph, Vec<DecisionRecord>)| {
                        serde_json::to_vec(&(crate::git_merge::render(&result.0), &result.1))
                            .unwrap()
                    };
                    assert_eq!(bytes(&actual), bytes(&expected));
                    let again = merge(&b, &o, &t, &shapes).unwrap();
                    assert_eq!(bytes(&actual), bytes(&again));
                    for record in actual.1 {
                        for (choice, values) in [
                            ("base", &record.base),
                            ("ours", &record.ours),
                            ("theirs", &record.theirs),
                        ] {
                            let resolved = resolve(&record, choice);
                            if values.len() <= record.max_count {
                                assert_eq!(resolved.unwrap(), *values);
                            } else {
                                assert!(resolved.is_err(), "over-bound {choice} accepted");
                            }
                        }
                        let union: BTreeSet<_> = record.ours.iter().chain(&record.theirs).collect();
                        assert_eq!(
                            resolve(&record, "both").is_ok(),
                            union.len() <= record.max_count
                        );
                        assert!(resolve(&record, "continue").is_err());
                    }
                }
            }
        }
    }
}

#[test]
fn convergent_continue_is_time_free_and_overrides_use_the_same_core() {
    let set = |values: &[&str]| values.iter().map(|v| (*v).to_owned()).collect();
    let base = set(&["\"base\""]);
    let agreed = set(&["\"agreed\""]);
    let input = input(&base, &agreed, &agreed, Some(1));
    let plan = plan_slot(&input, None, "").unwrap();
    assert_eq!(plan.change, crate::merge_core::Change::Convergent);
    assert!(plan.receive.is_empty() && plan.decide.is_empty());
    assert_eq!(
        project(&input, Some(&Decision::Continue)).unwrap(),
        (agreed, false)
    );
    assert_eq!(
        project(&input, Some(&Decision::Base)).unwrap(),
        (base, false)
    );
    let explicit = set(&["\"chosen\""]);
    assert_eq!(
        project(&input, Some(&Decision::Values(explicit.clone()))).unwrap(),
        (explicit, false)
    );
    let too_many = Decision::Values(set(&["\"a\"", "\"b\""]));
    assert!(project(&input, Some(&too_many)).is_err());
}

#[test]
fn held_slot_does_not_erase_a_clean_slot_from_the_inspectable_pack() {
    let b = graph(1);
    let o = graph(2);
    let mut t = graph(4);
    t.extend(parse_graph("<urn:test:clean> <urn:test:q> \"new\" .", "clean").unwrap());
    let shapes = "@prefix sh: <http://www.w3.org/ns/shacl#> . \
                  [] sh:path <urn:test:p> ; sh:maxCount 1 .";
    let (output, conflicts) = merge(&b, &o, &t, shapes).unwrap();
    let mut expected = b;
    expected.extend(parse_graph("<urn:test:clean> <urn:test:q> \"new\" .", "expected").unwrap());
    assert_eq!(output, expected);
    assert_eq!(conflicts.len(), 1);
}
