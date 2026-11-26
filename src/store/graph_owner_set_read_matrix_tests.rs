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

/// How each file's term space is arranged before the composed read
/// (dearing ruling n0wc.5 c30, option A: read-only ATTACH needs disjoint spaces).
#[derive(Clone, Copy)]
enum Spaces {
    /// Legacy main space 0, routed set file respaced to its own space 3.
    Disjoint,
    /// R1a: both files respaced into the SAME space 5.
    Overlapping,
    /// R1c: routed file DECLARES space 3 but its ids still lie in space 0.
    DeclaredOnly,
    /// R1d: routed file never given a space (space 0, like main).
    SpaceZero,
}

/// Rewrite `name.db` in place into `space` (respace writes a fresh file).
fn respaced(root: &Path, name: &str, store: Store, space: i64) -> Store {
    drop(store);
    let path = root.join(format!("{name}.db"));
    let tmp = root.join(format!("{name}.respaced.db"));
    crate::store::respace::respace_file(&path, &tmp, space).unwrap();
    std::fs::rename(&tmp, &path).unwrap();
    // Same ownership step as `file`: a rewritten file must be adopted again.
    Store::adopt(path.to_str().unwrap()).unwrap();
    Store::open(path.to_str().unwrap()).unwrap()
}

fn file_digest(root: &Path, name: &str) -> String {
    use sha2::{Digest, Sha256};
    format!(
        "{:x}",
        Sha256::digest(std::fs::read(root.join(format!("{name}.db"))).unwrap())
    )
}

fn matrix() -> Matrix {
    matrix_with(Spaces::Disjoint)
}

fn matrix_with(spaces: Spaces) -> Matrix {
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
    let (legacy, routed) = match spaces {
        Spaces::Disjoint => {
            let routed = respaced(root, "application", routed, 3);
            // ADVERSARY 1 under disjoint spaces: the colliding pair now differs.
            assert_ne!(routed.lookup("urn:right").unwrap(), Some(left));
            (legacy, routed)
        }
        Spaces::Overlapping => (
            respaced(root, "legacy", legacy, 5),
            respaced(root, "application", routed, 5),
        ),
        Spaces::DeclaredOnly => {
            routed
                .conn
                .execute("UPDATE term_spaces SET space = 3 WHERE local = 1", [])
                .unwrap();
            (legacy, routed)
        }
        Spaces::SpaceZero => (legacy, routed),
    };
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
    let output = m
        .dataset
        .read_attached(graphs, deadline(), |reads, checkout| {
            let view = policy
                .authorize(&identity, ReadOperation::Query, None, 2)?
                .open_attached_cut(reads, checkout, permit, 2, deadline())?;
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

/// R1a/R1c/R1d: a composed mount whose term spaces cannot be proven disjoint
/// refuses BEFORE execution, names the reason, and never renumbers on read.
fn assert_space_refusal(spaces: Spaces, row: &str) {
    let m = matrix_with(spaces);
    let root = m._dir.path().to_path_buf();
    let before = (
        file_digest(&root, "legacy"),
        file_digest(&root, "application"),
    );
    let result = run(
        &m,
        &BOTH,
        &format!(
            "SELECT ?v {} WHERE {{ GRAPH <{APP}> {{ <urn:right> <urn:value> ?v }} }}",
            named(&BOTH)
        ),
    );
    let error = match result {
        Ok(answer) => panic!("{row}: composed mount answered instead of refusing: {answer}"),
        Err(error) => error.to_string(),
    };
    assert!(
        error.contains("term space"),
        "{row}: refusal must name the term-space reason, got: {error}"
    );
    assert_eq!(
        before,
        (
            file_digest(&root, "legacy"),
            file_digest(&root, "application")
        ),
        "{row}: a refused read must not renumber or touch either file"
    );
}

#[test]
fn r1a_files_sharing_one_term_space_refuse_before_mount() {
    assert_space_refusal(Spaces::Overlapping, "R1a");
}

// Mutant: skipping the measured min/max id check at mount (trusting
// term_spaces alone) must turn this row red.
#[test]
fn r1c_a_declared_space_whose_real_ids_overlap_refuses_at_mount() {
    assert_space_refusal(Spaces::DeclaredOnly, "R1c");
}

// The paired mutant: trusting the declared space alone answers again.
#[test]
fn r1c_mutant_trusting_the_declared_space_lets_overlapping_ids_mount() {
    use crate::read_authorization::mutants::{Mutant, with};
    let m = matrix_with(Spaces::DeclaredOnly);
    let answer = with(Mutant::AttachTrustDeclaredSpace, || {
        run(
            &m,
            &BOTH,
            &format!(
                "SELECT ?v {} WHERE {{ GRAPH <{APP}> {{ <urn:right> <urn:value> ?v }} }}",
                named(&BOTH)
            ),
        )
    });
    assert!(
        answer.is_ok(),
        "mutant did not disable the id-range check: {answer:?}"
    );
}

#[test]
fn r1d_a_space_zero_set_file_refuses_composed_mount_and_is_never_renumbered() {
    assert_space_refusal(Spaces::SpaceZero, "R1d");
}

// R1b: disjoint spaces; the formerly colliding pair (fixture) never joins.
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
    let result = m.dataset.read_attached(&BOTH, short, |reads, checkout| {
        let view = policy
            .authorize(&identity, ReadOperation::Query, None, 2)?
            .open_attached_cut(reads, checkout, permit, 2, short)?;
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

// P1/P2 (dearing n0wc.5 c32): a pooled composition connection starts clean.
// X mounts the routed file; Y on the SAME connection must see only `main`
// and an empty TEMP, and must still answer exactly.
#[test]
fn p2_a_reused_composition_connection_carries_nothing_from_the_previous_read() {
    let m = matrix();
    let x = run(
        &m,
        &BOTH,
        &format!(
            "SELECT ?v {} WHERE {{ GRAPH <{APP}> {{ <urn:right> <urn:value> ?v }} }}",
            named(&BOTH)
        ),
    )
    .unwrap();
    assert_eq!(values(&x, "v"), ["must-not-join"]);
    assert_eq!(m.dataset.composition_idle(), 1, "X returned its connection");
    let checkout = m.dataset.composition_checkout().unwrap();
    assert!(
        checkout.is_clean_now().unwrap(),
        "X left state on the connection"
    );
    drop(checkout);
    // Y on the same connection: legacy-only, then both again.
    let y = run(
        &m,
        &[REFS],
        &format!("SELECT ?o FROM NAMED <{REFS}> WHERE {{ GRAPH <{REFS}> {{ <urn:pointer> <urn:pointsTo> ?o }} }}"),
    )
    .unwrap();
    assert_eq!(values(&y, "o"), ["urn:left"]);
    let again = run(
        &m,
        &BOTH,
        &format!(
            "SELECT ?v {} WHERE {{ GRAPH <{APP}> {{ <urn:right> <urn:value> ?v }} }}",
            named(&BOTH)
        ),
    )
    .unwrap();
    assert_eq!(values(&again, "v"), ["must-not-join"]);
}

// Skip-detach alone: checkout verification still discards the dirty
// connection, so the next read gets a fresh, clean one.
#[test]
fn p1_checkout_verification_discards_a_connection_left_attached() {
    use crate::read_authorization::mutants::{Mutant, with};
    let m = matrix();
    let query = format!(
        "SELECT ?v {} WHERE {{ GRAPH <{APP}> {{ <urn:right> <urn:value> ?v }} }}",
        named(&BOTH)
    );
    with(Mutant::CompositionSkipDetach, || run(&m, &BOTH, &query)).unwrap();
    let checkout = m.dataset.composition_checkout().unwrap();
    assert!(
        checkout.is_clean_now().unwrap(),
        "dirty connection was reused"
    );
}

// Both mutants: the leftover attachment is visible to the next checkout.
// This is what proves the test above can SEE leakage.
#[test]
fn p2_mutant_skipping_detach_and_verification_leaks_the_attachment() {
    use crate::read_authorization::mutants::{Mutant, with};
    let m = matrix();
    let query = format!(
        "SELECT ?v {} WHERE {{ GRAPH <{APP}> {{ <urn:right> <urn:value> ?v }} }}",
        named(&BOTH)
    );
    with(Mutant::CompositionSkipDetach, || run(&m, &BOTH, &query)).unwrap();
    let checkout = with(Mutant::CompositionSkipCheckoutVerify, || {
        m.dataset.composition_checkout()
    })
    .unwrap();
    assert!(
        !checkout.is_clean_now().unwrap(),
        "mutant left no trace: the leakage test cannot observe leakage"
    );
}

// P3: the snapshot is pinned at checkout, under the fences. A write that
// commits after the fences are released (inside the read callback) is NOT
// seen by this read, and IS seen by the next one.
#[test]
fn p3_a_write_after_the_fences_release_is_invisible_to_the_pinned_read() {
    let m = matrix();
    let query = format!(
        "SELECT ?v {} WHERE {{ GRAPH <{APP}> {{ <urn:late> <urn:value> ?v }} }}",
        named(&BOTH)
    );
    let (policy, identity) = authority(&BOTH);
    let policy = policy.snapshot().unwrap();
    let permit = m.admission.try_reserve().unwrap();
    let output = m
        .dataset
        .read_attached(&BOTH, deadline(), |reads, checkout| {
            {
                let mut writer = m.dataset.routed[APP].owner.writer.lock();
                write(&mut writer, APP, "<urn:late> <urn:value> \"after-cut\" .");
            }
            let view = policy
                .authorize(&identity, ReadOperation::Query, None, 2)?
                .open_attached_cut(reads, checkout, permit, 2, deadline())?;
            view.query(&query, false, 2)
        })
        .unwrap();
    let pinned: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert!(
        rows(&pinned).is_empty(),
        "read saw a post-cut write: {pinned}"
    );
    let next = run(&m, &BOTH, &query).unwrap();
    assert_eq!(values(&next, "v"), ["after-cut"]);
}

// The first composed reader has a BOUNDED attached set (design: "Over-limit
// queries refuse before execution"). A selection needing more attachments
// than the bound refuses before any ATTACH, naming the limit; a selection
// within the bound still answers.
#[test]
fn an_attached_set_over_the_bound_refuses_before_any_attach() {
    let mut m = matrix();
    m.dataset.set_composition_attach_limit_for_test(0);
    let refused = run(
        &m,
        &BOTH,
        &format!(
            "SELECT ?v {} WHERE {{ GRAPH <{APP}> {{ <urn:right> <urn:value> ?v }} }}",
            named(&BOTH)
        ),
    )
    .expect_err("an over-limit attached set answered");
    assert!(
        refused.to_string().contains("attachment limit"),
        "refusal must name the attachment limit: {refused}"
    );
    // Within the bound (no attachment needed): the legacy-only read answers.
    let legacy = run(
        &m,
        &[REFS],
        &format!("SELECT ?o FROM NAMED <{REFS}> WHERE {{ GRAPH <{REFS}> {{ <urn:pointer> <urn:pointsTo> ?o }} }}"),
    )
    .unwrap();
    assert_eq!(values(&legacy, "o"), ["urn:left"]);
}

// Composition memory plan (aegis-n0wc.5 c37): every ATTACHED schema gets an
// explicit, bounded page cache, so the per-connection worst case is the main
// cache plus max_attached x ATTACHED_CACHE_KIB, not a compiled default.
#[test]
fn every_attached_schema_has_the_bounded_page_cache() {
    let m = matrix();
    let sizes = m
        .dataset
        .read_attached(&BOTH, deadline(), |_, checkout| {
            checkout.cache_sizes_for_test()
        })
        .unwrap();
    assert_eq!(sizes[0], ("main".to_owned(), -32000), "{sizes:?}");
    let attached: Vec<_> = sizes[1..].to_vec();
    assert!(
        !attached.is_empty(),
        "the selection attached nothing: {sizes:?}"
    );
    for (schema, size) in attached {
        assert_eq!(
            size,
            -crate::store::composition::ATTACHED_CACHE_KIB,
            "{schema} cache_size"
        );
    }
}
