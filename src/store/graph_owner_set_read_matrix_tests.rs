//! Phase 2 composed-reader adversary matrix (aegis-n0wc.5,
//! docs/design/per-graph-writable-stores.md "Terms, links and query
//! semantics"). Black-box through the authorized composed view only, so the
//! same assertions bind the current reader and any cut-aware replacement.
//! A legacy file (ROOT + REFS) and a routed set file (APP + PROV) each carry
//! adversarial data: numeric term IDs that collide for DIFFERENT IRIs, the same
//! IRI under DIFFERENT IDs, look-alike literals, duplicate triples, a property
//! path that crosses files, and an unselected graph that must never be seen.
use super::*;

const XSD_INT: &str = "http://www.w3.org/2001/XMLSchema#integer";
const HIDDEN: &str = "urn:hidden";
const MISSING: &str = "urn:never-created";

struct Matrix {
    _dir: tempfile::TempDir,
    dataset: GraphSetDataset,
    admission: ComposedReadAdmission,
}

/// Intern padding terms until the store's NEXT allocation is `target`.
fn pad_to(store: &Store, target: i64, tag: &str) {
    let mut i = 0;
    loop {
        let probe = format!("urn:pad:{tag}:{i}");
        let id = store.intern(&probe).unwrap();
        if id + 1 >= target {
            assert_eq!(id + 1, target, "padding overshot the target term id");
            return;
        }
        i += 1;
    }
}

fn matrix() -> Matrix {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let mut legacy = file(root, "legacy", &[REFS]);
    let mut routed = file(root, "application", &[APP, PROV, HIDDEN]);
    // Push the legacy allocator well past anything the routed file has used.
    for i in 0..96 {
        legacy.intern(&format!("urn:legacy-pad:{i}")).unwrap();
    }
    write(
        &mut legacy,
        ROOT_GRAPH_IRI,
        "<urn:service> <urn:tracks> <urn:reference> .",
    );
    write(
        &mut legacy,
        REFS,
        &format!(
            "<urn:shared> <urn:label> \"chat\"@en .
             <urn:shared> <urn:count> \"42\"^^<{XSD_INT}> .
             <urn:dup> <urn:p> <urn:o> .
             <urn:a> <urn:next> <urn:b> .
             <urn:pointer> <urn:pointsTo> <urn:left> ."
        ),
    );
    // ADVERSARY 1: a DIFFERENT IRI in the routed file gets the SAME numeric id.
    let left = legacy.lookup("urn:left").unwrap().unwrap();
    pad_to(&routed, left, "align");
    assert_eq!(routed.intern("urn:right").unwrap(), left);
    write(
        &mut routed,
        APP,
        &format!(
            "<urn:right> <urn:value> \"must-not-join\" .
             <urn:shared> <urn:label> \"chat\"@fr .
             <urn:shared> <urn:count> \"42\"^^<{XSD_INT}> .
             <urn:shared> <urn:countText> \"42\" .
             <urn:dup> <urn:p> <urn:o> .
             <urn:b> <urn:next> <urn:c> ."
        ),
    );
    write(
        &mut routed,
        PROV,
        "<urn:write> <urn:describes> <urn:shared> .",
    );
    write(&mut routed, HIDDEN, "<urn:shared> <urn:label> \"secret\" .");
    // ADVERSARY 2: the SAME IRI has DIFFERENT ids in the two files.
    assert_ne!(
        legacy.lookup("urn:shared").unwrap(),
        routed.lookup("urn:shared").unwrap()
    );
    let catalog = Arc::new(
        GraphSetCatalog::new(
            7,
            &legacy.store_id().unwrap(),
            vec![GraphSet {
                owner: pin(&routed, "application", APP),
                graphs: vec![APP.into(), PROV.into()],
            }],
            Budget {
                max_owners: 1,
                queued_per_owner: 1,
                readers_per_owner: 1,
                max_queued: 1,
                max_connections: 3,
            },
            4,
        )
        .unwrap()
        .with_legacy_graphs(
            pin(&legacy, "legacy", ROOT_GRAPH_IRI),
            vec![ROOT_GRAPH_IRI.into(), REFS.into()],
        )
        .unwrap(),
    );
    let admission =
        ComposedReadAdmission::for_graph_sets(catalog.clone(), 512, 3, 1, reservation()).unwrap();
    let owner = GraphSetOwner::prepare(
        catalog.clone(),
        root,
        APP,
        routed,
        "policy-v1",
        admission.allocation(),
    )
    .unwrap();
    let dataset = GraphSetDataset::prepare(
        catalog,
        root,
        legacy,
        vec![owner],
        "policy-v1",
        admission.allocation(),
    )
    .unwrap();
    Matrix {
        _dir: dir,
        dataset,
        admission,
    }
}

/// Run one SPARQL query over exactly `graphs`, granted exactly `graphs`.
fn run(m: &Matrix, graphs: &[&str], sparql: &str) -> Result<serde_json::Value> {
    let (policy, identity) = authority(graphs);
    let policy = policy.snapshot().unwrap();
    let permit = m.admission.try_reserve()?;
    let output = m.dataset.read(graphs, deadline(), |reads| {
        let view = policy
            .authorize(&identity, ReadOperation::Query, None, 2)?
            .open_composed_cut(reads, permit, 2, deadline())?;
        view.query(sparql, false, 2)
    })?;
    Ok(serde_json::from_slice(&output).unwrap())
}

fn rows(result: &serde_json::Value) -> Vec<serde_json::Value> {
    result["results"]["bindings"].as_array().unwrap().clone()
}

fn values(result: &serde_json::Value, var: &str) -> Vec<String> {
    let mut out: Vec<String> = rows(result)
        .iter()
        .map(|row| row[var]["value"].as_str().unwrap().to_owned())
        .collect();
    out.sort();
    out
}

fn named(graphs: &[&str]) -> String {
    graphs
        .iter()
        .map(|g| format!("FROM NAMED <{g}> "))
        .collect()
}

const BOTH: [&str; 2] = [REFS, APP];

#[test]
fn different_iris_with_the_same_numeric_id_never_join_across_files() {
    let m = matrix();
    let joined = run(
        &m,
        &BOTH,
        &format!(
            "SELECT ?v {} WHERE {{ GRAPH <{REFS}> {{ ?p <urn:pointsTo> ?o }} \
             GRAPH <{APP}> {{ ?o <urn:value> ?v }} }}",
            named(&BOTH)
        ),
    )
    .unwrap();
    assert!(rows(&joined).is_empty(), "joined by numeric id: {joined}");
    // Positive twin: the routed fact is there under its own IRI.
    let direct = run(
        &m,
        &BOTH,
        &format!(
            "SELECT ?v {} WHERE {{ GRAPH <{APP}> {{ <urn:right> <urn:value> ?v }} }}",
            named(&BOTH)
        ),
    )
    .unwrap();
    assert_eq!(values(&direct, "v"), ["must-not-join"]);
    // And the legacy side resolves to its own IRI, not the routed one.
    let legacy = run(
        &m,
        &BOTH,
        &format!(
            "SELECT ?o {} WHERE {{ GRAPH <{REFS}> {{ <urn:pointer> <urn:pointsTo> ?o }} }}",
            named(&BOTH)
        ),
    )
    .unwrap();
    assert_eq!(values(&legacy, "o"), ["urn:left"]);
}

#[test]
fn the_same_iri_under_different_ids_is_one_rdf_term_for_join_distinct_and_grouping() {
    let m = matrix();
    let join = run(
        &m,
        &BOTH,
        &format!(
            "SELECT ?s {} WHERE {{ GRAPH <{REFS}> {{ ?s <urn:count> ?a }} \
             GRAPH <{APP}> {{ ?s <urn:count> ?b }} }}",
            named(&BOTH)
        ),
    )
    .unwrap();
    assert_eq!(values(&join, "s"), ["urn:shared"]);
    let distinct = run(
        &m,
        &BOTH,
        &format!(
            "SELECT DISTINCT ?s {} WHERE {{ {{ GRAPH <{REFS}> {{ ?s <urn:label> ?l }} }} \
             UNION {{ GRAPH <{APP}> {{ ?s <urn:label> ?l }} }} }}",
            named(&BOTH)
        ),
    )
    .unwrap();
    assert_eq!(values(&distinct, "s"), ["urn:shared"]);
    let grouped = run(
        &m,
        &BOTH,
        &format!(
            "SELECT ?s (COUNT(*) AS ?n) {} WHERE {{ {{ GRAPH <{REFS}> {{ ?s <urn:count> ?c }} }} \
             UNION {{ GRAPH <{APP}> {{ ?s <urn:count> ?c }} }} }} GROUP BY ?s",
            named(&BOTH)
        ),
    )
    .unwrap();
    let groups = rows(&grouped);
    assert_eq!(groups.len(), 1, "one group per RDF term: {grouped}");
    assert_eq!(groups[0]["n"]["value"], "2");
}

#[test]
fn literal_identity_keeps_language_and_datatype_across_files() {
    let m = matrix();
    let same_typed = run(
        &m,
        &BOTH,
        &format!(
            "SELECT ?n {} WHERE {{ GRAPH <{REFS}> {{ <urn:shared> <urn:count> ?n }} \
             GRAPH <{APP}> {{ <urn:shared> <urn:count> ?n }} }}",
            named(&BOTH)
        ),
    )
    .unwrap();
    let typed = rows(&same_typed);
    assert_eq!(typed.len(), 1, "equal typed literals join: {same_typed}");
    assert_eq!(typed[0]["n"]["datatype"], XSD_INT);
    let typed_vs_plain = run(
        &m,
        &BOTH,
        &format!(
            "SELECT ?n {} WHERE {{ GRAPH <{REFS}> {{ <urn:shared> <urn:count> ?n }} \
             GRAPH <{APP}> {{ <urn:shared> <urn:countText> ?n }} }}",
            named(&BOTH)
        ),
    )
    .unwrap();
    assert!(
        rows(&typed_vs_plain).is_empty(),
        "\"42\"^^xsd:integer joined \"42\": {typed_vs_plain}"
    );
    let languages = run(
        &m,
        &BOTH,
        &format!(
            "SELECT ?l {} WHERE {{ GRAPH <{REFS}> {{ <urn:shared> <urn:label> ?l }} \
             GRAPH <{APP}> {{ <urn:shared> <urn:label> ?l }} }}",
            named(&BOTH)
        ),
    )
    .unwrap();
    assert!(rows(&languages).is_empty(), "@en joined @fr: {languages}");
    let both = run(
        &m,
        &BOTH,
        &format!(
            "SELECT ?l {} WHERE {{ {{ GRAPH <{REFS}> {{ <urn:shared> <urn:label> ?l }} }} \
             UNION {{ GRAPH <{APP}> {{ <urn:shared> <urn:label> ?l }} }} }}",
            named(&BOTH)
        ),
    )
    .unwrap();
    let mut tags: Vec<String> = rows(&both)
        .iter()
        .map(|r| r["l"]["xml:lang"].as_str().unwrap_or("").to_owned())
        .collect();
    tags.sort();
    assert_eq!(tags, ["en", "fr"]);
}

#[test]
fn bag_multiplicity_is_preserved_separately_from_rdf_identity() {
    let m = matrix();
    let union = format!(
        "{} WHERE {{ {{ GRAPH <{REFS}> {{ ?s <urn:p> <urn:o> }} }} \
         UNION {{ GRAPH <{APP}> {{ ?s <urn:p> <urn:o> }} }} }}",
        named(&BOTH)
    );
    let bag = run(&m, &BOTH, &format!("SELECT ?s {union}")).unwrap();
    assert_eq!(values(&bag, "s"), ["urn:dup", "urn:dup"], "bag lost a row");
    let set = run(&m, &BOTH, &format!("SELECT DISTINCT ?s {union}")).unwrap();
    assert_eq!(values(&set, "s"), ["urn:dup"]);
    let count = run(&m, &BOTH, &format!("SELECT (COUNT(*) AS ?n) {union}")).unwrap();
    assert_eq!(rows(&count)[0]["n"]["value"], "2");
}

#[test]
fn property_paths_cross_files_only_through_an_explicit_merged_default_graph() {
    let m = matrix();
    let merged = run(
        &m,
        &BOTH,
        &format!("SELECT ?x FROM <{REFS}> FROM <{APP}> WHERE {{ <urn:a> <urn:next>+ ?x }}"),
    )
    .unwrap();
    assert_eq!(values(&merged, "x"), ["urn:b", "urn:c"]);
    let one_graph = run(
        &m,
        &BOTH,
        &format!(
            "SELECT ?x {} WHERE {{ GRAPH <{REFS}> {{ <urn:a> <urn:next>+ ?x }} }}",
            named(&BOTH)
        ),
    )
    .unwrap();
    assert_eq!(
        values(&one_graph, "x"),
        ["urn:b"],
        "path leaked across graphs"
    );
}

#[test]
fn per_graph_aggregates_count_exactly_the_selected_members() {
    let m = matrix();
    let selected = [ROOT_GRAPH_IRI, REFS, APP, PROV];
    let counts = run(
        &m,
        &selected,
        &format!(
            "SELECT ?g (COUNT(*) AS ?n) FROM <{ROOT_GRAPH_IRI}> {} WHERE {{ GRAPH ?g {{ ?s ?p ?o }} }} \
             GROUP BY ?g ORDER BY ?g",
            named(&[REFS, APP, PROV])
        ),
    )
    .unwrap();
    let by_graph: std::collections::BTreeMap<String, String> = rows(&counts)
        .iter()
        .map(|r| {
            (
                r["g"]["value"].as_str().unwrap().to_owned(),
                r["n"]["value"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    let expected: std::collections::BTreeMap<String, String> = [
        (APP.to_owned(), "6".to_owned()),
        (PROV.to_owned(), "1".to_owned()),
        (REFS.to_owned(), "5".to_owned()),
    ]
    .into();
    assert_eq!(by_graph, expected, "{counts}");
}

#[test]
fn unselected_and_nonexistent_graphs_are_indistinguishable_and_never_enumerated() {
    let m = matrix();
    let enumerated = run(
        &m,
        &BOTH,
        &format!(
            "SELECT DISTINCT ?g {} WHERE {{ GRAPH ?g {{ ?s ?p ?o }} }}",
            named(&BOTH)
        ),
    )
    .unwrap();
    assert_eq!(values(&enumerated, "g"), [APP, REFS]);
    // A graph that exists in the routed file but is not selected answers
    // exactly like one that exists nowhere: no existence oracle.
    let probe = |graph: &str| {
        run(
            &m,
            &BOTH,
            &format!(
                "SELECT (COUNT(*) AS ?n) {} WHERE {{ GRAPH <{graph}> {{ ?s ?p ?o }} }}",
                named(&BOTH)
            ),
        )
        .map(|v| v.to_string())
        .map_err(|e| e.to_string())
    };
    assert_eq!(probe(HIDDEN), probe(MISSING));
    let secret = run(
        &m,
        &BOTH,
        &format!(
            "SELECT ?l {} WHERE {{ GRAPH ?g {{ <urn:shared> <urn:label> ?l }} }}",
            named(&BOTH)
        ),
    )
    .unwrap();
    assert!(
        !values(&secret, "l").iter().any(|l| l == "secret"),
        "unselected graph content leaked: {secret}"
    );
}

#[test]
fn a_selected_member_that_cannot_be_captured_is_an_error_never_an_empty_answer() {
    let m = matrix();
    // Hold the routed owner's writer so a strict capture cannot fence it.
    let held = m.dataset.routed[APP].owner.writer.lock();
    let (policy, identity) = authority(&BOTH);
    let policy = policy.snapshot().unwrap();
    let permit = m.admission.try_reserve().unwrap();
    let short = Instant::now() + Duration::from_millis(300);
    let result = m.dataset.read(&BOTH, short, |reads| {
        let view = policy
            .authorize(&identity, ReadOperation::Query, None, 2)?
            .open_composed_cut(reads, permit, 2, short)?;
        view.query(
            &format!(
                "SELECT * {} WHERE {{ GRAPH ?g {{ ?s ?p ?o }} }}",
                named(&BOTH)
            ),
            false,
            2,
        )
    });
    drop(held);
    assert!(
        result.is_err(),
        "an uncapturable member produced a successful (partial or empty) answer"
    );
    // Positive twin: once released the same dataset answers.
    let after = run(
        &m,
        &BOTH,
        &format!(
            "SELECT * {} WHERE {{ GRAPH ?g {{ ?s ?p ?o }} }}",
            named(&BOTH)
        ),
    )
    .unwrap();
    assert_eq!(rows(&after).len(), 11);
}
