use super::*;
const PREFIX: &str = "@prefix sh: <http://www.w3.org/ns/shacl#> . @prefix qp: <https://w3id.org/intigraph/quipu/ns#> . @prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .";
fn shape(target: &str) -> String {
    format!(
        "{PREFIX} <urn:node> {target}; sh:property <urn:prop> . <urn:prop> sh:path <urn:count>; qp:currentValuePolicy \"sum\" ."
    )
}
fn count(shapes: &str, data: &str) -> usize {
    bindings(shapes, data, "urn:graph", "urn:alice")
        .unwrap()
        .len()
}
#[test]
fn class_targets_use_selected_graph_and_subclasses_without_global_promotion() {
    let shapes = shape("sh:targetClass <urn:Person>");
    let data =
        format!("{PREFIX} <urn:alice> a <urn:Child> . <urn:Child> rdfs:subClassOf <urn:Person> .");
    assert_eq!(count(&shapes, &data), 1);
    assert_eq!(count(&shapes, "<urn:bob> a <urn:Person> ."), 0);
    assert_eq!(count(&shapes, ""), 0);
    let found = bindings(&shapes, &data, "urn:other-graph", "urn:alice").unwrap();
    assert_eq!(found[0].graph, "urn:other-graph");
    assert_eq!(found[0].subject, "urn:alice");
    assert_eq!(found[0].declaration.shape, "urn:prop");
    assert_eq!(found[0].declaration.policy, Policy::Sum);
    let bare = format!("{PREFIX} <urn:prop> sh:path <urn:count>; qp:currentValuePolicy \"sum\" .");
    assert_eq!(count(&bare, &data), 0);
}
#[test]
fn all_core_targets_and_implicit_classes_resolve() {
    for (target, data) in [
        ("sh:targetNode <urn:alice>", ""),
        (
            "sh:targetSubjectsOf <urn:edge>",
            "<urn:alice> <urn:edge> <urn:bob> .",
        ),
        (
            "sh:targetObjectsOf <urn:edge>",
            "<urn:bob> <urn:edge> <urn:alice> .",
        ),
        ("a rdfs:Class", "<urn:alice> a <urn:node> ."),
    ] {
        assert_eq!(count(&shape(target), data), 1, "{target}");
    }
    assert_eq!(count(&shape("sh:targetNode <urn:bob>"), ""), 0);
    let direct = format!(
        "{PREFIX} <urn:prop> sh:targetNode <urn:alice>; sh:path <urn:count>; qp:currentValuePolicy \"sum\" ."
    );
    assert_eq!(count(&direct, ""), 1);
}
#[test]
fn deactivation_ambiguity_and_custom_targets_fail_closed() {
    let shape = shape("sh:targetNode <urn:alice>");
    assert_eq!(
        count(&format!("{shape} <urn:node> sh:deactivated true ."), ""),
        0
    );
    assert_eq!(
        count(&format!("{shape} <urn:prop> sh:deactivated true ."), ""),
        0
    );
    assert_eq!(
        count(&format!("{shape} <urn:node> sh:deactivated false ."), ""),
        1
    );
    for bad in [
        "<urn:node> sh:target <urn:custom> .",
        "<urn:node> sh:targetClass \"bad\" .",
        "<urn:node> sh:deactivated \"true\" .",
        "<urn:node> sh:deactivated true, false .",
        "<urn:other> sh:targetNode <urn:alice>; sh:property <urn:second> . <urn:second> sh:path <urn:count>; qp:currentValuePolicy \"max\" .",
    ] {
        assert!(
            bindings(&format!("{shape} {bad}"), "", "urn:g", "urn:alice").is_err(),
            "{bad}"
        );
    }
    assert!(bindings(&shape, "", "not graph", "urn:alice").is_err());
    assert!(bindings(&shape, "", "urn:g", "_:blank").is_err());
    let blank = format!("{PREFIX} [] sh:path <urn:p>; qp:currentValuePolicy \"sum\" .");
    assert!(declarations(&blank).is_err());
}
