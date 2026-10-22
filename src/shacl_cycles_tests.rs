use super::{find_shape_cycle, refuse_cyclic_shapes};

const PREFIXES: &str = "@prefix sh: <http://www.w3.org/ns/shacl#> .\n\
                        @prefix ex: <http://example.org/> .\n";

fn shapes(body: &str) -> String {
    format!("{PREFIXES}{body}")
}

#[test]
fn self_recursion_through_a_property_shape_is_a_cycle() {
    // The aegis-ahu0 repro shape: ex:L validates ex:link values against itself.
    let ttl = shapes(
        "ex:S a sh:NodeShape ; sh:targetClass ex:T ;
             sh:property [ sh:path ex:link ; sh:node ex:L ] .
         ex:L a sh:NodeShape ;
             sh:property [ sh:path ex:link ; sh:node ex:L ] .",
    );
    let cycle = find_shape_cycle(&ttl).unwrap().expect("cycle");
    assert_eq!(cycle.first(), cycle.last());
    assert!(cycle.iter().any(|n| n == "http://example.org/L"));
}

#[test]
fn mutual_recursion_through_a_logical_list_is_a_cycle() {
    let ttl = shapes(
        "ex:A a sh:NodeShape ; sh:property [ sh:path ex:p ; sh:node ex:B ] .
         ex:B a sh:NodeShape ; sh:property [ sh:path ex:q ; sh:or ( [ sh:node ex:A ] ) ] .",
    );
    assert!(find_shape_cycle(&ttl).unwrap().is_some());
}

#[test]
fn qualified_value_shape_recursion_is_a_cycle() {
    let ttl = shapes(
        "ex:Q a sh:NodeShape ; sh:property [ sh:path ex:r ;
             sh:qualifiedValueShape ex:Q ; sh:qualifiedMinCount 1 ] .",
    );
    assert!(find_shape_cycle(&ttl).unwrap().is_some());
}

#[test]
fn nested_acyclic_shapes_are_accepted() {
    // The prod pattern: a shape referencing another that never refers back.
    let ttl = shapes(
        "ex:Lead a sh:NodeShape ; sh:property [ sh:path ex:leadFor ; sh:node ex:Keeper ] .
         ex:Keeper a sh:NodeShape ; sh:property [ sh:path ex:name ; sh:minCount 1 ] ;
             sh:or ( [ sh:property [ sh:path ex:a ; sh:minCount 1 ] ]
                     [ sh:property [ sh:path ex:b ; sh:minCount 1 ] ] ) .",
    );
    assert_eq!(find_shape_cycle(&ttl).unwrap(), None);
    refuse_cyclic_shapes(&ttl).unwrap();
}

#[test]
fn a_shape_shared_by_two_parents_is_not_a_cycle() {
    // A diamond: two shapes reference the same leaf. Revisiting a FINISHED node
    // is not a cycle; only reaching a node still on the path is.
    let ttl = shapes(
        "ex:P1 a sh:NodeShape ; sh:node ex:Leaf .
         ex:P2 a sh:NodeShape ; sh:node ex:Leaf .
         ex:Top a sh:NodeShape ; sh:and ( ex:P1 ex:P2 ) .
         ex:Leaf a sh:NodeShape ; sh:property [ sh:path ex:x ; sh:maxCount 1 ] .",
    );
    assert_eq!(find_shape_cycle(&ttl).unwrap(), None);
}

#[test]
fn refusal_names_the_cycle_and_does_not_validate() {
    let ttl = shapes("ex:L a sh:NodeShape ; sh:property [ sh:path ex:link ; sh:node ex:L ] .");
    let err = refuse_cyclic_shapes(&ttl).unwrap_err().to_string();
    assert!(
        err.contains("recursive SHACL shapes are not supported"),
        "{err}"
    );
    assert!(err.contains("http://example.org/L"), "{err}");
}

#[test]
fn a_cyclic_rdf_list_does_not_hang_the_walk() {
    // A malformed list whose rest points back at itself must terminate.
    let ttl = format!(
        "{PREFIXES}@prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .
         ex:A a sh:NodeShape ; sh:or ex:cell .
         ex:cell rdf:first ex:B ; rdf:rest ex:cell .
         ex:B a sh:NodeShape ; sh:property [ sh:path ex:p ; sh:minCount 1 ] ."
    );
    assert_eq!(find_shape_cycle(&ttl).unwrap(), None);
}

const RECURSIVE: &str = "@prefix sh: <http://www.w3.org/ns/shacl#> .
@prefix ex: <http://example.org/> .
ex:S a sh:NodeShape ; sh:targetClass ex:T ;
    sh:property [ sh:path ex:link ; sh:node ex:L ] .
ex:L a sh:NodeShape ;
    sh:property [ sh:path ex:link ; sh:node ex:L ] .";

#[test]
fn loading_a_recursive_set_is_refused_and_the_registry_is_unchanged() {
    let store = crate::Store::open_in_memory().unwrap();
    let err = store
        .load_shapes("recursive", RECURSIVE, "2026-10-22T00:00:00Z")
        .unwrap_err()
        .to_string();
    assert!(err.contains("recursive SHACL shapes"), "{err}");
    assert!(
        store.list_shapes().unwrap().is_empty(),
        "nothing was stored"
    );
}

#[test]
fn a_cycle_spanning_two_loaded_sets_is_refused_at_the_second_load() {
    let store = crate::Store::open_in_memory().unwrap();
    let first = "@prefix sh: <http://www.w3.org/ns/shacl#> . @prefix ex: <http://example.org/> .
        ex:A a sh:NodeShape ; sh:property [ sh:path ex:p ; sh:node ex:B ] .";
    let second = "@prefix sh: <http://www.w3.org/ns/shacl#> . @prefix ex: <http://example.org/> .
        ex:B a sh:NodeShape ; sh:property [ sh:path ex:q ; sh:node ex:A ] .";
    store
        .load_shapes("first", first, "2026-10-22T00:00:00Z")
        .unwrap();
    assert!(
        store
            .load_shapes("second", second, "2026-10-22T00:00:01Z")
            .is_err()
    );
    let names: Vec<_> = store
        .list_shapes()
        .unwrap()
        .into_iter()
        .map(|r| r.0)
        .collect();
    assert_eq!(names, ["first"]);
}

#[test]
fn replacing_a_set_is_judged_without_its_old_version() {
    // Reloading the same name replaces it; the old version must not combine with
    // the new one into a phantom cycle.
    let store = crate::Store::open_in_memory().unwrap();
    let v1 = "@prefix sh: <http://www.w3.org/ns/shacl#> . @prefix ex: <http://example.org/> .
        ex:A a sh:NodeShape ; sh:property [ sh:path ex:p ; sh:node ex:B ] .";
    let v2 = "@prefix sh: <http://www.w3.org/ns/shacl#> . @prefix ex: <http://example.org/> .
        ex:B a sh:NodeShape ; sh:property [ sh:path ex:q ; sh:node ex:A ] .";
    store
        .load_shapes("same", v1, "2026-10-22T00:00:00Z")
        .unwrap();
    store
        .load_shapes("same", v2, "2026-10-22T00:00:01Z")
        .unwrap();
}

#[cfg(feature = "shacl")]
#[test]
fn a_deep_chain_with_a_recursive_request_shape_is_refused_not_aborted() {
    // The ahu0 failure: a per-request recursive shape over a long linked chain
    // recursed once per hop. Now refused before validation, nothing written.
    let mut store = crate::Store::open_in_memory().unwrap();
    let mut data = String::from("@prefix ex: <http://example.org/> .\nex:n0 a ex:T .\n");
    for i in 0..20_000 {
        data.push_str(&format!("ex:n{i} ex:link ex:n{} .\n", i + 1));
    }
    let result = crate::tool_knot(
        &mut store,
        &serde_json::json!({"turtle": data, "shapes": RECURSIVE,
                            "timestamp": "2026-10-22T00:00:00Z"}),
    );
    // The refusal must be THIS one, not an unrelated gate (vocabulary, parse)
    // that would pass the test without exercising the recursion guard.
    assert!(
        matches!(&result, Err(e) if e.to_string().contains("recursive SHACL shapes")),
        "recursive request shape must be refused by the cycle guard: {result:?}"
    );
    let rows = crate::sparql::query(&store, "ASK { <http://example.org/n0> ?p ?o }").unwrap();
    assert!(
        !format!("{rows:?}").contains("true"),
        "nothing written: {rows:?}"
    );
}
