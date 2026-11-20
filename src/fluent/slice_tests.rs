// aegis-550z: the indexed binding slice must select exactly what the whole
// graph selects, must not grow with unrelated facts, and must refuse above
// its bound.
const SLICE_PREFIX: &str = "@prefix sh: <http://www.w3.org/ns/shacl#> . @prefix qp: <https://w3id.org/intigraph/quipu/ns#> . @prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .";
const SLICE_TS: &str = "2026-11-19T07:00:00Z";

fn slice_shape(target: &str) -> String {
    format!(
        "{SLICE_PREFIX} <urn:node> {target}; sh:property <urn:prop> . <urn:prop> sh:path <urn:count>; qp:currentValuePolicy \"sum\" ."
    )
}

fn slice_store(turtle: &str) -> Store {
    let mut store = Store::open_in_memory().unwrap();
    crate::rdf::ingest_rdf_to_graph(
        &mut store,
        turtle.as_bytes(),
        oxrdfio::RdfFormat::Turtle,
        None,
        SLICE_TS,
        None,
        None,
        0,
    )
    .unwrap();
    store
}

fn ntriples(store: &Store, facts: &[Fact]) -> String {
    String::from_utf8(
        crate::rdf::serialize_facts(store, facts, oxrdfio::RdfFormat::NTriples).unwrap(),
    )
    .unwrap()
}

fn noise(n: usize) -> String {
    (0..n)
        .map(|i| format!("<urn:noise:{i}> <urn:noise-edge> <urn:noise:{}> . <urn:noise:{i}> a <urn:Noise> .\n", i + 1))
        .collect()
}

#[test]
fn slice_selection_equals_whole_graph_selection_for_every_target_kind() {
    let cases = [
        ("sh:targetNode <urn:alice>", ""),
        (
            "sh:targetClass <urn:Person>",
            "<urn:alice> a <urn:Child> . <urn:Child> rdfs:subClassOf <urn:Kid> . <urn:Kid> rdfs:subClassOf <urn:Person> .",
        ),
        ("sh:targetClass <urn:Person>", "<urn:bob> a <urn:Person> ."),
        (
            "sh:targetSubjectsOf <urn:edge>",
            "<urn:alice> <urn:edge> <urn:bob> .",
        ),
        (
            "sh:targetObjectsOf <urn:edge>",
            "<urn:bob> <urn:edge> <urn:alice> .",
        ),
        (
            "sh:targetObjectsOf <urn:edge>",
            "<urn:alice> <urn:edge> <urn:alice> .",
        ),
        ("a rdfs:Class", "<urn:alice> a <urn:node> ."),
        (
            "a rdfs:Class",
            "<urn:alice> a <urn:Sub> . <urn:Sub> rdfs:subClassOf <urn:node> .",
        ),
    ];
    let mut selected = 0;
    for (target, data) in cases {
        let shapes = slice_shape(target);
        let store = slice_store(&format!("{SLICE_PREFIX} {data}\n{}", noise(200)));
        let whole = ntriples(&store, &store.current_facts_in_graph(0).unwrap());
        for subject in ["urn:alice", "urn:bob", "urn:noise:3", "urn:absent"] {
            let slice = ntriples(&store, &binding_facts(&store, 0, subject).unwrap());
            let old = super::targets::bindings(&shapes, &whole, "urn:graph", subject).unwrap();
            let new = super::targets::bindings(&shapes, &slice, "urn:graph", subject).unwrap();
            assert_eq!(old, new, "{target} / {data} / {subject}");
            selected += new.len();
        }
    }
    // Non-vacuity: the cases above really select (alice x7, bob x1).
    assert_eq!(selected, 8);
}

#[test]
fn slice_does_not_grow_with_unrelated_facts() {
    let data = "<urn:alice> a <urn:Child> . <urn:Child> rdfs:subClassOf <urn:Person> . <urn:bob> <urn:edge> <urn:alice> .";
    let small = slice_store(&format!("{SLICE_PREFIX} {data}"));
    let large = slice_store(&format!("{SLICE_PREFIX} {data}\n{}", noise(5_000)));
    let a = binding_facts(&small, 0, "urn:alice").unwrap().len();
    let b = binding_facts(&large, 0, "urn:alice").unwrap().len();
    assert_eq!(a, 3);
    assert_eq!(a, b);
    assert!(large.current_facts_in_graph(0).unwrap().len() > 10_000);
}

#[test]
fn slice_guard_refuses_above_the_bound_and_accepts_at_it() {
    let classes = |n: usize| -> String {
        (0..n)
            .map(|i| {
                format!(
                    "<urn:c:{i}> <http://www.w3.org/2000/01/rdf-schema#subClassOf> <urn:c:{}> .\n",
                    i + 1
                )
            })
            .collect()
    };
    let at = slice_store(&classes(BINDING_SLICE_LIMIT));
    assert_eq!(
        binding_facts(&at, 0, "urn:alice").unwrap().len(),
        BINDING_SLICE_LIMIT
    );
    let over = slice_store(&classes(BINDING_SLICE_LIMIT + 1));
    let error = binding_facts(&over, 0, "urn:alice")
        .unwrap_err()
        .to_string();
    assert!(error.contains("aegis-550z"), "{error}");
}

/// Acceptance measurement (aegis-550z): append + current on a graph holding
/// `QUIPU_550Z_FACTS` (default 1,000,000) unrelated facts within 2x of a 1k-fact
/// graph. Run explicitly: `cargo test --features full slice_latency -- --ignored --nocapture`.
#[cfg(feature = "shacl")]
#[test]
#[ignore = "measurement: builds a 1M-fact store"]
fn slice_latency_independent_of_unrelated_facts() {
    use super::Scalar;
    use crate::transaction_auth::{Identity, with_identity};
    let identity = Identity {
        principal: "urn:writer:authenticated".into(),
        credential_id: Some("test-credential".into()),
        auth_class: "named_bearer".into(),
        attestation: Default::default(),
    };
    let fluents = include_str!("../../shapes/fluents-v1.ttl");
    let declaration = format!(
        "<urn:count-shape> <http://www.w3.org/ns/shacl#path> <urn:count>; <http://www.w3.org/ns/shacl#targetNode> <urn:subject>; <{}currentValuePolicy> \"sum\" .",
        crate::namespace::QUIPU_FLUENT
    );
    let big: usize = std::env::var("QUIPU_550Z_FACTS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1_000_000);
    let measure = |unrelated: usize| -> (u128, u128, u128, u128) {
        let mut store = slice_store(&noise(unrelated / 2));
        store
            .load_shapes("quipu-fluents-v1", fluents, SLICE_TS)
            .unwrap();
        store
            .load_shapes("test-declaration", &declaration, SLICE_TS)
            .unwrap();
        let (mut appends, mut reads) = (Vec::new(), Vec::new());
        with_identity(Some(identity.clone()), || {
            for i in 0..20 {
                let request = super::writer::Append {
                    fluent: "urn:fluent:count".into(),
                    event: format!("urn:event:{i}"),
                    subject: "urn:subject".into(),
                    predicate: "urn:count".into(),
                    property_shape: "urn:count-shape".into(),
                    as_of: "2026-11-19T07:00:00Z".into(),
                    value: Scalar::Integer(1),
                };
                let t = std::time::Instant::now();
                store.append_fluent_native(&request, 0).unwrap();
                appends.push(t.elapsed().as_micros());
                let t = std::time::Instant::now();
                store.fluent_current_native(0, "urn:fluent:count").unwrap();
                reads.push(t.elapsed().as_micros());
            }
        });
        appends.sort_unstable();
        reads.sort_unstable();
        (
            appends[10],
            *appends.last().unwrap(),
            reads[10],
            *reads.last().unwrap(),
        )
    };
    let small = measure(1_000);
    let large = measure(big);
    eprintln!("550z latency us (append p50,max, current p50,max): 1k {small:?} / {big} {large:?}");
    assert!(
        large.0 <= 2 * small.0.max(1_000),
        "append p50 {large:?} vs {small:?}"
    );
    assert!(
        large.2 <= 2 * small.2.max(1_000),
        "current p50 {large:?} vs {small:?}"
    );
    assert!(
        large.1 <= 2 * small.1.max(5_000),
        "append max {large:?} vs {small:?}"
    );
    assert!(
        large.3 <= 2 * small.3.max(5_000),
        "current max {large:?} vs {small:?}"
    );
}
