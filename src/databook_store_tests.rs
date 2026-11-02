use super::*;

const G: &str = "https://example.org/graphs/people";
const T0: &str = "2026-11-01T00:00:00Z";
const T1: &str = "2026-11-01T01:00:00Z";
const DATA: &str = r#"@prefix ex: <https://example.org/> .
ex:ada a ex:Person ;
    ex:name "Ada"@en ;
    ex:born "1815-12-10"^^<http://www.w3.org/2001/XMLSchema#date> ;
    ex:note """two
lines with ``` inside""" ;
    ex:knows [ ex:name "a blank friend" ] .
"#;

fn seeded() -> Store {
    let mut store = Store::open_in_memory().unwrap();
    let g = store.graph_create(G).unwrap();
    crate::rdf::ingest_rdf_to_graph(
        &mut store,
        DATA.as_bytes(),
        RdfFormat::Turtle,
        None,
        T0,
        None,
        None,
        g,
    )
    .unwrap();
    store
}

fn export_opts(graph: Option<&str>) -> ExportOptions<'_> {
    ExportOptions {
        graph,
        as_of: None,
        id: None,
        title: None,
        version: "1.0.0",
        with_shapes: false,
        timestamp: T1,
    }
}

fn import_opts() -> ImportOptions<'static> {
    ImportOptions {
        graph: None,
        with_shapes: false,
        timestamp: T1,
        actor: Some("test"),
        source: None,
    }
}

/// The graph's content as canonical N-Triples, blank-node labels erased.
fn content(store: &Store, iri: &str) -> Vec<String> {
    let (nt, _) = crate::rdf::export_rdf_subset(store, RdfFormat::NTriples, Some(iri)).unwrap();
    let mut lines: Vec<String> = String::from_utf8(nt)
        .unwrap()
        .lines()
        .map(|l| {
            l.split(' ')
                .map(|w| if w.starts_with("_:") { "_:b" } else { w })
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect();
    lines.sort();
    lines
}

#[test]
fn a_graph_round_trips_through_a_databook() {
    let source = seeded();
    let text = export(&source, &export_opts(Some(G))).unwrap();
    let book = databook::parse(&text).unwrap();
    assert!(book.warnings.is_empty(), "{:?}", book.warnings);
    assert_eq!(book.header.named_graph.as_deref(), Some(G));
    assert_eq!(book.header.triple_count, Some(6));
    assert!(book.header.id.starts_with("urn:quipu:databook:sha256:"));

    let mut target = Store::open_in_memory().unwrap();
    let report = import(&mut target, &text, &import_opts()).unwrap();
    assert_eq!(report.graphs.len(), 1);
    assert_eq!((report.graphs[0].0.as_str(), report.graphs[0].2), (G, 6));
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    assert_eq!(content(&target, G), content(&source, G));
    assert_eq!(content(&target, G).len(), 6);
}

#[test]
fn the_import_transaction_names_the_databook() {
    let text = export(&seeded(), &export_opts(Some(G))).unwrap();
    let mut target = Store::open_in_memory().unwrap();
    let report = import(&mut target, &text, &import_opts()).unwrap();
    let tx = report.graphs[0].1;
    let (actor, source): (Option<String>, Option<String>) = target
        .conn
        .query_row(
            "SELECT actor, source FROM transactions WHERE id = ?1",
            [tx],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(actor.as_deref(), Some("test"));
    assert_eq!(source, Some(format!("databook:{}@1.0.0", report.id)));
}

#[test]
fn export_is_deterministic_and_content_addressed() {
    let a = export(&seeded(), &export_opts(Some(G))).unwrap();
    let b = export(&seeded(), &export_opts(Some(G))).unwrap();
    assert_eq!(a, b);
    let mut other = seeded();
    let g = other.lookup(G).unwrap().unwrap();
    crate::rdf::ingest_rdf_to_graph(
        &mut other,
        "<https://example.org/x> <https://example.org/y> \"z\" .".as_bytes(),
        RdfFormat::NTriples,
        None,
        T1,
        None,
        None,
        g,
    )
    .unwrap();
    let c = export(&other, &export_opts(Some(G))).unwrap();
    assert_ne!(
        databook::parse(&a).unwrap().header.id,
        databook::parse(&c).unwrap().header.id
    );
}

#[test]
fn as_of_export_reads_the_past() {
    let mut store = seeded();
    let before = store.latest_tx_id().unwrap();
    let g = store.lookup(G).unwrap().unwrap();
    crate::rdf::ingest_rdf_to_graph(
        &mut store,
        "<https://example.org/later> <https://example.org/p> \"new\" .".as_bytes(),
        RdfFormat::NTriples,
        None,
        T1,
        None,
        None,
        g,
    )
    .unwrap();
    let mut opts = export_opts(Some(G));
    opts.as_of = Some(AsOf {
        tx: Some(before),
        valid_at: None,
    });
    let past = export(&store, &opts).unwrap();
    assert!(!past.contains("https://example.org/later"));
    assert!(past.contains(&format!("as of transaction {before}")));
    let now = export(&store, &export_opts(Some(G))).unwrap();
    assert!(now.contains("https://example.org/later"));
}

#[test]
fn root_exports_and_unknown_graphs_refuse() {
    let mut store = Store::open_in_memory().unwrap();
    crate::rdf::ingest_rdf_to_graph(
        &mut store,
        DATA.as_bytes(),
        RdfFormat::Turtle,
        None,
        T0,
        None,
        None,
        0,
    )
    .unwrap();
    let text = export(&store, &export_opts(None)).unwrap();
    assert!(text.contains(&format!("named_graph: \"{ROOT_GRAPH_IRI}\"")));
    let mut target = Store::open_in_memory().unwrap();
    let report = import(&mut target, &text, &import_opts()).unwrap();
    assert_eq!(report.graphs[0].0, ROOT_GRAPH_IRI);
    assert_eq!(target.current_facts_in_graph(0).unwrap().len(), 6);
    assert!(export(&store, &export_opts(Some("https://example.org/none"))).is_err());
}

#[test]
fn a_tampered_block_is_refused_before_any_write() {
    let text = export(&seeded(), &export_opts(Some(G))).unwrap();
    let tampered = text.replace("\"Ada\"@en", "\"Eve\"@en");
    assert_ne!(tampered, text);
    let mut target = Store::open_in_memory().unwrap();
    let head = target.latest_tx_id().unwrap();
    let err = import(&mut target, &tampered, &import_opts()).unwrap_err();
    assert!(err.to_string().contains("quipu:sha256"), "{err}");
    assert_eq!(target.latest_tx_id().unwrap(), head);
}

const TWO_GRAPHS: &str = "---\nid: urn:test:two\ntitle: Two\ntype: databook\nversion: 1.0.0\ncreated: 2026-11-01\ngraph:\n  triple_count: 3\nprocess:\n  transformer: t\n---\n\n```trig\n<urn:g:a> { <urn:s> <urn:p> \"a\" . }\n<urn:g:b> { <urn:s> <urn:p> \"b\" . <urn:s> <urn:p> \"c\" . }\n```\n";

#[test]
fn trig_graph_names_are_honoured() {
    let mut store = Store::open_in_memory().unwrap();
    let report = import(&mut store, TWO_GRAPHS, &import_opts()).unwrap();
    let graphs: Vec<_> = report
        .graphs
        .iter()
        .map(|(g, _, n)| (g.as_str(), *n))
        .collect();
    assert_eq!(graphs, [("urn:g:a", 1), ("urn:g:b", 2)]);
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);

    let mut overridden = Store::open_in_memory().unwrap();
    let opts = ImportOptions {
        graph: Some("urn:g:all"),
        ..import_opts()
    };
    let report = import(&mut overridden, TWO_GRAPHS, &opts).unwrap();
    assert_eq!(
        report.graphs,
        [("urn:g:all".to_string(), report.graphs[0].1, 3)]
    );
}

#[test]
fn a_bad_block_anywhere_writes_nothing() {
    let broken = TWO_GRAPHS.replace("```\n", "```\n\n```turtle\n<urn:s> <urn:p> .\n```\n");
    let mut store = Store::open_in_memory().unwrap();
    let head = store.latest_tx_id().unwrap();
    assert!(import(&mut store, &broken, &import_opts()).is_err());
    assert_eq!(store.latest_tx_id().unwrap(), head);
    assert!(
        store.committed_graph_id("urn:g:a").unwrap().is_none(),
        "no graph registered"
    );
}

#[test]
fn queries_and_updates_are_reported_never_run() {
    let text = TWO_GRAPHS.replace(
        "```\n",
        "```\n\n```sparql\n<!-- databook:id: q -->\nSELECT * { ?s ?p ?o }\n```\n\n```sparql-update\nDROP ALL\n```\n\n```shacl\n<urn:s> <urn:p> <urn:o> .\n```\n\n```json-ld\n{}\n```\n",
    );
    let mut store = Store::open_in_memory().unwrap();
    let report = import(&mut store, &text, &import_opts()).unwrap();
    assert_eq!(report.graphs.len(), 2, "DROP ALL did not run");
    assert_eq!(report.skipped.len(), 4, "{:?}", report.skipped);
    assert!(report.skipped[0].contains("sparql block `q`"));
    assert!(report.skipped[1].contains("never executed"));
    assert!(report.skipped[2].contains("--with-shapes"));
    assert!(store.list_shapes().unwrap().is_empty());
}

#[test]
fn a_triple_count_mismatch_warns_and_still_loads() {
    let text = TWO_GRAPHS.replace("triple_count: 3", "triple_count: 9");
    let mut store = Store::open_in_memory().unwrap();
    let report = import(&mut store, &text, &import_opts()).unwrap();
    assert_eq!(report.warnings.len(), 1);
    assert!(report.warnings[0].contains("9"));
    assert_eq!(report.graphs.len(), 2);
}

#[cfg(feature = "shacl")]
#[test]
fn shapes_travel_when_asked() {
    const SHAPES: &str = "@prefix sh: <http://www.w3.org/ns/shacl#> .\n@prefix ex: <https://example.org/> .\nex:PersonShape a sh:NodeShape ; sh:targetClass ex:Person ;\n    sh:property [ sh:path ex:name ; sh:minCount 1 ] .\n";
    // A `&mut` binding: `load_shapes` takes `&mut self` once C1 mapping settlement lands.
    let source = &mut seeded();
    source.load_shapes("people", SHAPES, T0).unwrap();
    let mut opts = export_opts(Some(G));
    opts.with_shapes = true;
    let text = export(source, &opts).unwrap();
    assert!(text.contains("```shacl"));
    assert!(text.contains("<!-- quipu:shapes-name: people -->"));

    let mut plain = Store::open_in_memory().unwrap();
    import(&mut plain, &text, &import_opts()).unwrap();
    assert!(plain.list_shapes().unwrap().is_empty());

    let mut target = Store::open_in_memory().unwrap();
    let report = import(
        &mut target,
        &text,
        &ImportOptions {
            with_shapes: true,
            ..import_opts()
        },
    )
    .unwrap();
    assert_eq!(report.shapes, ["people"]);
    assert_eq!(target.list_shapes().unwrap()[0].1, SHAPES);
}

#[test]
fn printed_blocks_are_reported_and_manifests_load() {
    let printed = TWO_GRAPHS.replace("```trig", "<!-- mode=printed -->\n```trig");
    let mut store = Store::open_in_memory().unwrap();
    let err = import(&mut store, &printed, &import_opts())
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("no graph data to import; skipped: trig block at line"),
        "{err}"
    );
    assert!(err.contains("marked for reading only"), "{err}");

    let manifest = TWO_GRAPHS
        .replace("```trig\n<urn:g:a> { <urn:s> <urn:p> \"a\" . }\n<urn:g:b> { <urn:s> <urn:p> \"b\" . <urn:s> <urn:p> \"c\" . }", "```manifest\n<urn:s> <urn:p> \"a\", \"b\", \"c\" .");
    let report = import(&mut store, &manifest, &import_opts()).unwrap();
    assert_eq!(
        report.graphs,
        [(ROOT_GRAPH_IRI.to_string(), report.graphs[0].1, 3)]
    );
}

#[cfg(feature = "shacl")]
#[test]
fn rdf_12_triple_terms_are_refused_by_block_and_register_nothing() {
    let text = TWO_GRAPHS.replace(
        "```trig\n<urn:g:a> { <urn:s> <urn:p> \"a\" . }\n<urn:g:b> { <urn:s> <urn:p> \"b\" . <urn:s> <urn:p> \"c\" . }",
        "<!-- databook:id: annotated -->\n<!-- databook:graph: urn:g:r -->\n```turtle12\n<urn:s> <urn:p> <urn:o> ~ <urn:r> {| <urn:by> \"x\" |} .",
    );
    let mut store = Store::open_in_memory().unwrap();
    let head = store.latest_tx_id().unwrap();
    let err = import(&mut store, &text, &import_opts())
        .unwrap_err()
        .to_string();
    assert!(err.contains("turtle12 block `annotated`"), "{err}");
    assert!(err.contains("urn:g:r"), "{err}");
    assert_eq!(store.latest_tx_id().unwrap(), head);
    assert!(store.committed_graph_id("urn:g:r").unwrap().is_none());
}

#[cfg(feature = "shacl")]
#[test]
fn a_shapes_only_databook_counts_its_shapes() {
    let shapes_only = |count: u64| {
        format!(
            "---\nid: urn:test:shapes\ntitle: S\ntype: databook\nversion: 1.0.0\ncreated: 2026-11-01\ngraph:\n  triple_count: {count}\nprocess:\n  transformer: t\n---\n\n<!-- databook:id: core -->\n```shacl\n@prefix sh: <http://www.w3.org/ns/shacl#> .\n<urn:S> a sh:NodeShape ; sh:targetClass <urn:C> .\n```\n"
        )
    };
    let opts = ImportOptions {
        with_shapes: true,
        ..import_opts()
    };
    let mut store = Store::open_in_memory().unwrap();
    let report = import(&mut store, &shapes_only(2), &opts).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    assert_eq!(report.shapes, ["core"]);
    let mut store = Store::open_in_memory().unwrap();
    let report = import(&mut store, &shapes_only(5), &opts).unwrap();
    assert_eq!(
        report.warnings,
        ["graph.triple_count says 5, the shacl blocks hold 2"]
    );
}

/// Two blocks with the same `_:x` payload, each with its own `databook:id`.
fn twin_blocks(graph_a: &str, graph_b: &str) -> String {
    format!(
        "---\nid: urn:test:twins\ntitle: Twins\ntype: databook\nversion: 1.0.0\ncreated: 2026-11-01\nprocess:\n  transformer: t\n---\n\n\
         <!-- databook:id: first -->\n<!-- databook:graph: {graph_a} -->\n```turtle\n_:x <urn:p> \"same\" .\n```\n\n\
         <!-- databook:id: second -->\n<!-- databook:graph: {graph_b} -->\n```turtle\n_:x <urn:p> \"same\" .\n```\n"
    )
}

/// The distinct subjects of `<urn:p>` facts in `graph`, as entity ids.
fn subjects(store: &Store, graph: &str) -> BTreeSet<i64> {
    let g = store.lookup(graph).unwrap().unwrap();
    let p = store.lookup("urn:p").unwrap().unwrap();
    store
        .current_facts_in_graph(g)
        .unwrap()
        .into_iter()
        .filter(|f| f.attribute == p)
        .map(|f| f.entity)
        .collect()
}

#[test]
fn identical_blocks_in_one_graph_keep_distinct_blank_nodes() {
    let text = twin_blocks("urn:g:one", "urn:g:one");
    let mut store = Store::open_in_memory().unwrap();
    import(&mut store, &text, &import_opts()).unwrap();
    let first = subjects(&store, "urn:g:one");
    assert_eq!(first.len(), 2, "two blocks, two blank nodes");

    // Re-importing the same document is deterministic: the same two nodes.
    import(&mut store, &text, &import_opts()).unwrap();
    assert_eq!(subjects(&store, "urn:g:one"), first);

    // Without ids the blocks are keyed by position, and still stay apart.
    let anonymous = text
        .replace("<!-- databook:id: first -->\n", "")
        .replace("<!-- databook:id: second -->\n", "");
    let mut store = Store::open_in_memory().unwrap();
    import(&mut store, &anonymous, &import_opts()).unwrap();
    assert_eq!(subjects(&store, "urn:g:one").len(), 2);
}

#[test]
fn identical_blocks_in_two_graphs_share_no_blank_node() {
    let mut store = Store::open_in_memory().unwrap();
    import(
        &mut store,
        &twin_blocks("urn:g:one", "urn:g:two"),
        &import_opts(),
    )
    .unwrap();
    let (one, two) = (subjects(&store, "urn:g:one"), subjects(&store, "urn:g:two"));
    assert_eq!((one.len(), two.len()), (1, 1));
    assert!(one.is_disjoint(&two), "{one:?} {two:?}");
}

#[test]
fn one_trig_block_shares_its_blank_node_across_graphs() {
    let text = "---\nid: urn:test:shared\ntitle: S\ntype: databook\nversion: 1.0.0\ncreated: 2026-11-01\nprocess:\n  transformer: t\n---\n\n```trig\n<urn:g:one> { _:x <urn:p> \"in one\" . }\n<urn:g:two> { _:x <urn:p> \"in two\" . _:y <urn:p> \"other\" . }\n```\n";
    let mut store = Store::open_in_memory().unwrap();
    import(&mut store, text, &import_opts()).unwrap();
    let (one, two) = (subjects(&store, "urn:g:one"), subjects(&store, "urn:g:two"));
    assert_eq!((one.len(), two.len()), (1, 2));
    assert!(one.is_subset(&two), "_:x is one node across both graphs");
    // And a different document with the same block content gets its own nodes.
    let mut other_doc = Store::open_in_memory().unwrap();
    import(
        &mut other_doc,
        &text.replace("urn:test:shared", "urn:test:other"),
        &import_opts(),
    )
    .unwrap();
    let mine: BTreeSet<_> = one.iter().map(|&e| store.resolve(e).unwrap()).collect();
    let theirs: BTreeSet<_> = subjects(&other_doc, "urn:g:one")
        .iter()
        .map(|&e| other_doc.resolve(e).unwrap())
        .collect();
    assert!(mine.is_disjoint(&theirs), "{mine:?} {theirs:?}");
}

#[test]
fn a_round_trip_keeps_blank_node_count() {
    let source = seeded();
    let text = export(&source, &export_opts(Some(G))).unwrap();
    let mut target = Store::open_in_memory().unwrap();
    import(&mut target, &text, &import_opts()).unwrap();
    let blanks = |store: &Store| {
        let g = store.lookup(G).unwrap().unwrap();
        store
            .current_facts_in_graph(g)
            .unwrap()
            .iter()
            .filter(|f| store.resolve(f.entity).unwrap().starts_with("_:"))
            .map(|f| f.entity)
            .collect::<BTreeSet<_>>()
            .len()
    };
    assert_eq!(blanks(&target), blanks(&source));
    assert_eq!(blanks(&target), 1);
}

#[test]
fn an_explicit_id_spelled_like_a_position_keeps_its_own_nodes() {
    // Block 0 is explicitly `#1`; block 1 is anonymous, so its position is 1.
    let text = "---\nid: urn:test:twins\ntitle: Twins\ntype: databook\nversion: 1.0.0\ncreated: 2026-11-01\nprocess:\n  transformer: t\n---\n\n\
        <!-- databook:id: #1 -->\n<!-- databook:graph: urn:g:one -->\n```turtle\n_:x <urn:p> \"same\" .\n```\n\n\
        <!-- databook:graph: urn:g:one -->\n```turtle\n_:x <urn:p> \"same\" .\n```\n";
    let mut store = Store::open_in_memory().unwrap();
    import(&mut store, text, &import_opts()).unwrap();
    let first = subjects(&store, "urn:g:one");
    assert_eq!(
        first.len(),
        2,
        "explicit `#1` and position 1 are different blocks"
    );
    import(&mut store, text, &import_opts()).unwrap();
    assert_eq!(
        subjects(&store, "urn:g:one"),
        first,
        "and stay stable on re-import"
    );
}
