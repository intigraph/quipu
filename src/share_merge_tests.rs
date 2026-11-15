use super::*;

const P_ONE: &str = "https://example.org/one";
const P_MANY: &str = "https://example.org/many";
const SHAPES: &str = r#"
@prefix sh: <http://www.w3.org/ns/shacl#> .
[] sh:path <https://example.org/one> ; sh:maxCount 1 .
[] sh:path <https://example.org/many> .
"#;

fn graph(lines: &[(&str, &str)]) -> Graph {
    let text = lines
        .iter()
        .map(|(p, o)| format!("<https://example.org/s> <{p}> {o} .\n"))
        .collect::<String>();
    parse_graph(&text, "test graph").unwrap()
}

fn values(graph: &Graph, predicate: &str) -> BTreeSet<String> {
    by_slot(graph)
        .get(&("<https://example.org/s>".into(), predicate.into()))
        .cloned()
        .unwrap_or_default()
}

#[test]
fn independent_multivalued_additions_union() {
    let base = graph(&[]);
    let ours = graph(&[(P_MANY, "\"ours\"")]);
    let theirs = graph(&[(P_MANY, "\"theirs\"")]);
    let (merged, conflicts) = merge_graphs(&base, &ours, &theirs, SHAPES).unwrap();
    assert!(conflicts.is_empty());
    assert_eq!(
        values(&merged, P_MANY),
        BTreeSet::from(["\"ours\"".into(), "\"theirs\"".into()])
    );
}

#[test]
fn divergent_functional_values_emit_decision_and_hold_base() {
    let base = graph(&[(P_ONE, "\"base\"")]);
    let ours = graph(&[(P_ONE, "\"ours\"")]);
    let theirs = graph(&[(P_ONE, "\"theirs\"")]);
    let (merged, conflicts) = merge_graphs(&base, &ours, &theirs, SHAPES).unwrap();
    assert_eq!(conflicts.len(), 1);
    assert_eq!(conflicts[0].max_count, 1);
    assert_eq!(values(&merged, P_ONE), BTreeSet::from(["\"base\"".into()]));
}

#[test]
fn functional_delete_replace_is_a_conflict() {
    let base = graph(&[(P_ONE, "\"base\"")]);
    let ours = graph(&[]);
    let theirs = graph(&[(P_ONE, "\"replacement\"")]);
    let (_, conflicts) = merge_graphs(&base, &ours, &theirs, SHAPES).unwrap();
    assert_eq!(conflicts.len(), 1);
}

#[test]
fn missing_parent_refuses_before_merge() {
    let mut store = Store::open_in_memory().unwrap();
    crate::share_scrub::seed_test_catalogue(&mut store);
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("incoming");
    crate::share::share(
        &store,
        dir.to_str().unwrap(),
        &crate::share::ShareOptions {
            no_shapes: true,
            ..Default::default()
        },
    )
    .unwrap();
    let error = status(&store, &dir).unwrap_err().to_string();
    assert!(error.contains("no parent_share"), "{error}");
}

#[test]
fn clean_reconnect_writes_one_tx_with_two_provenance_parents() {
    let root = tempfile::tempdir().unwrap();
    let mut source = Store::open_in_memory().unwrap();
    crate::share_scrub::seed_test_catalogue(&mut source);
    source.load_shapes("merge", SHAPES, "2026-08-29").unwrap();
    crate::rdf::ingest_rdf(
        &mut source,
        graph(&[(P_ONE, "\"base\"")])
            .iter()
            .map(|t| format!("{t} .\n"))
            .collect::<String>()
            .as_bytes(),
        RdfFormat::NTriples,
        None,
        "2026-08-29T00:00:00Z",
        None,
        Some("base"),
    )
    .unwrap();
    let base_dir = root.path().join("base");
    let base_manifest = crate::share::share(
        &source,
        base_dir.to_str().unwrap(),
        &crate::share::ShareOptions::default(),
    )
    .unwrap();
    crate::rdf::ingest_rdf(
        &mut source,
        format!("<https://example.org/s> <{P_MANY}> \"theirs\" .\n").as_bytes(),
        RdfFormat::NTriples,
        None,
        "2026-08-29T00:01:00Z",
        None,
        Some("theirs"),
    )
    .unwrap();
    let incoming_dir = root.path().join("incoming");
    let incoming_manifest = crate::share::share(
        &source,
        incoming_dir.to_str().unwrap(),
        &crate::share::ShareOptions {
            parent_share: Some(base_manifest.share_id),
            ..Default::default()
        },
    )
    .unwrap();

    let mut local = Store::open_in_memory().unwrap();

    crate::share_scrub::seed_test_catalogue(&mut local);
    local.load_shapes("merge", SHAPES, "2026-08-29").unwrap();
    crate::rdf::ingest_rdf(&mut local, format!("<https://example.org/s> <{P_ONE}> \"base\" .\n<https://example.org/s> <{P_MANY}> \"ours\" .\n").as_bytes(), RdfFormat::NTriples, None, "2026-08-29T00:02:00Z", None, Some("ours")).unwrap();
    let result = merge(
        &mut local,
        &incoming_dir,
        "2026-08-29T00:03:00Z",
        Some("reviewer"),
    )
    .unwrap();
    assert_eq!(result.outcome, "merged");
    assert_eq!(result.asserted, 1);
    assert_eq!(result.provenance_parents[1], incoming_manifest.share_id);
    let tx = local
        .get_transaction(result.tx_id.unwrap())
        .unwrap()
        .unwrap();
    let source = tx.source.unwrap();
    assert!(source.contains(&result.provenance_parents[0]));
    assert!(source.contains(&result.provenance_parents[1]));
    let (bytes, _) = crate::rdf::export_rdf_subset(&local, RdfFormat::NTriples, None).unwrap();
    let text = String::from_utf8(bytes).unwrap();
    assert!(text.contains("\"ours\""));
    assert!(text.contains("\"theirs\""));
    let e = local.lookup("https://example.org/s").unwrap().unwrap();
    let a = local.lookup(P_MANY).unwrap().unwrap();
    let incoming = read_share(&incoming_dir).unwrap();
    let received = local
        .entity_facts(e)
        .unwrap()
        .into_iter()
        .find(|f| f.attribute == a && f.value == crate::types::Value::Str("theirs".into()))
        .unwrap();
    assert_eq!(received.valid_from, incoming.manifest.created_at);
    let before = local.latest_tx_id().unwrap();
    let repeated = merge(
        &mut local,
        &incoming_dir,
        "2099-01-10T00:00:00Z",
        Some("reviewer"),
    )
    .unwrap();
    assert_eq!(repeated.outcome, "already-applied");
    assert_eq!(repeated.tx_id, result.tx_id);
    assert_eq!(local.latest_tx_id().unwrap(), before);
}
