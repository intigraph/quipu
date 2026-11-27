use super::*;

fn reach(text: &str) -> UpdateGraphs {
    let update = crate::sparql_structure::parse_update(spargebra::SparqlParser::new(), text)
        .unwrap()
        .unwrap();
    update_graphs(&update)
}

fn named(list: &[&str]) -> BTreeSet<String> {
    list.iter().map(|s| (*s).to_owned()).collect()
}

/// The Seeds item-write shape: templates and a revision CAS, all in GRAPH.
#[test]
fn a_seeds_cas_update_names_every_graph_and_no_default() {
    let r = reach(
        "DELETE { GRAPH <urn:p> { <urn:i> <urn:rev> ?r } } \
         INSERT { GRAPH <urn:p> { <urn:i> <urn:rev> 2 } \
                  GRAPH <urn:p#seeds-provenance> { <urn:w> <urn:of> <urn:i> } } \
         WHERE { GRAPH <urn:p> { <urn:i> <urn:rev> ?r FILTER(?r = 1) } \
                 FILTER NOT EXISTS { GRAPH <urn:p#seeds-ephemeral> { <urn:i> <urn:x> ?y } } }",
    );
    assert_eq!(
        r,
        UpdateGraphs {
            named: named(&["urn:p", "urn:p#seeds-ephemeral", "urn:p#seeds-provenance"]),
            default: false,
            open: false,
        }
    );
}

#[test]
fn a_where_graph_read_is_named_even_when_nothing_is_written_there() {
    let r = reach(
        "INSERT { GRAPH <urn:a> { <urn:s> <urn:p> 1 } } WHERE { GRAPH <urn:b> { ?s ?p ?o } }",
    );
    assert_eq!(r.named, named(&["urn:a", "urn:b"]));
    assert!(!r.default && !r.open);
}

#[test]
fn default_graph_data_and_patterns_are_reported() {
    assert!(reach("INSERT DATA { <urn:s> <urn:p> 1 }").default);
    let r = reach("INSERT { GRAPH <urn:a> { ?s <urn:p> 1 } } WHERE { ?s <urn:q> ?o }");
    assert!(r.default, "a WHERE triple outside GRAPH reads ROOT");
}

#[test]
fn unfixed_reach_is_open() {
    for text in [
        "INSERT { GRAPH <urn:a> { ?s <urn:p> 1 } } WHERE { GRAPH ?g { ?s ?p ?o } }",
        "INSERT { GRAPH ?g { <urn:s> <urn:p> 1 } } WHERE { GRAPH ?g { ?s ?p ?o } }",
        "WITH <urn:a> INSERT { ?s <urn:p> 1 } WHERE { ?s ?p ?o }",
        "INSERT { GRAPH <urn:a> { ?s <urn:p> 1 } } USING <urn:b> WHERE { ?s ?p ?o }",
        "CLEAR GRAPH <urn:a>",
        "DROP GRAPH <urn:a>",
        "INSERT { GRAPH <urn:a> { ?s <urn:p> 1 } } WHERE { SERVICE <http://x> { ?s ?p ?o } }",
    ] {
        assert!(reach(text).open, "{text}");
    }
}

#[test]
fn exists_inside_a_filter_is_walked() {
    let r = reach(
        "INSERT { GRAPH <urn:a> { <urn:s> <urn:p> 1 } } \
         WHERE { GRAPH <urn:a> { ?s ?p ?o } FILTER EXISTS { GRAPH <urn:c> { ?s ?q ?z } } }",
    );
    assert_eq!(r.named, named(&["urn:a", "urn:c"]));
}

#[test]
fn management_targets_are_named_and_open() {
    for text in [
        "CLEAR GRAPH <urn:a>",
        "DROP SILENT GRAPH <urn:a>",
        "CREATE GRAPH <urn:a>",
        "LOAD <http://x/doc> INTO GRAPH <urn:a>",
    ] {
        let r = reach(text);
        assert!(r.open, "{text}");
        assert_eq!(r.named, named(&["urn:a"]), "{text}");
    }
    let all = reach("CLEAR ALL");
    assert!(all.open && all.named.is_empty());
}
