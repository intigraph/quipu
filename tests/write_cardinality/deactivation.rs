use super::{TS, batch, values};
use quipu::{Store, Value};

#[test]
fn deactivated_shapes_skip_unsupported_cardinality_paths_before_admission() {
    for (node, property) in [(true, false), (true, true)] {
        let shapes = format!(
            r#"@prefix ex: <https://example.org/d6s/> .
@prefix sh: <http://www.w3.org/ns/shacl#> .
ex:S a sh:NodeShape; sh:targetClass ex:C; sh:deactivated {node};
 sh:property [sh:path (ex:first ex:value); sh:maxCount 1; sh:deactivated {property}]."#
        );
        let data = r#"<https://example.org/d6s/item>
<http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <https://example.org/d6s/C> ."#;
        assert!(quipu::validate_shapes(&shapes, data).unwrap().conforms);
        let mut store = Store::open_in_memory().unwrap();
        for (value, typed) in [("A", true), ("B", false)] {
            let datums = batch(&store, value, typed);
            store
                .with_write_shapes(Some(&shapes), |s| s.transact(&datums, TS, None, None))
                .unwrap_or_else(|error| {
                    panic!("disabled node={node}, property={property}: {error}")
                });
        }
        assert_eq!(values(&store, 0).len(), 2);
    }
}

#[test]
fn deactivated_nested_node_does_not_impose_its_property_path() {
    let shapes = r#"@prefix ex: <https://example.org/d6s/> .
@prefix sh: <http://www.w3.org/ns/shacl#> .
ex:S a sh:NodeShape; sh:targetClass ex:C;
 sh:node [sh:deactivated true; sh:property [sh:path (ex:first ex:value); sh:maxCount 1]]."#;
    let data = r#"<https://example.org/d6s/item>
<http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <https://example.org/d6s/C> ."#;
    assert!(quipu::validate_shapes(shapes, data).unwrap().conforms);
    let mut store = Store::open_in_memory().unwrap();
    let datums = batch(&store, "A", true);
    store
        .with_write_shapes(Some(shapes), |s| s.transact(&datums, TS, None, None))
        .unwrap();
    assert_eq!(values(&store, 0), vec![Value::Str("A".into())]);
}

#[test]
fn inactive_parent_does_not_disable_an_independently_active_property() {
    for active in [
        "ex:T a sh:NodeShape; sh:targetClass ex:C; sh:property ex:P.",
        "ex:P sh:targetNode ex:item.",
        "ex:T a sh:NodeShape; sh:targetClass ex:C; sh:node [sh:property ex:P].",
        "ex:T a sh:NodeShape; sh:targetClass ex:C; sh:and ([sh:property ex:P]).",
        "ex:T a sh:NodeShape; sh:targetClass ex:C; sh:or ([sh:property ex:P]).",
        "ex:T a sh:NodeShape; sh:targetClass ex:C; sh:xone ([sh:property ex:P]).",
        "ex:T a sh:NodeShape; sh:targetClass ex:C; sh:not [sh:property ex:P].",
        "ex:T a sh:NodeShape; sh:targetClass ex:C; sh:property [sh:path ex:child; sh:qualifiedValueShape [sh:property ex:P]; sh:qualifiedMinCount 0].",
    ] {
        let shapes = format!(
            r#"@prefix ex: <https://example.org/d6s/> .
@prefix sh: <http://www.w3.org/ns/shacl#> .
ex:S a sh:NodeShape; sh:targetClass ex:C; sh:deactivated true; sh:property ex:P.
ex:P a sh:PropertyShape; sh:path (ex:first ex:value); sh:maxCount 1.
{active}"#
        );
        let mut store = Store::open_in_memory().unwrap();
        let datums = batch(&store, "A", true);
        let result =
            store.with_write_shapes(Some(&shapes), |s| s.transact(&datums, TS, None, None));
        assert!(
            matches!(result, Err(quipu::Error::CannotVerify(_))),
            "active reference {active}: {result:?}"
        );
        assert!(values(&store, 0).is_empty());
    }
}
