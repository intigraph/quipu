//! Compare the enrolled HTTP owner with an immutable, single-file RDF baseline.
use super::*;
use quipu::read_authorization::{
    ComposedReadAdmission, ComposedReadReservation, OwnerReadPolicy, ReadGrant, ReadIdentity,
    ReadOperation,
};
use quipu::{schema::ROOT_GRAPH_IRI, sparql, w3c};

const ROOT: &str = "<urn:shared> <urn:root> \"root\"; <urn:link> <urn:next>; \
    <urn:number> \"7\"^^<http://www.w3.org/2001/XMLSchema#integer> . \
    <urn:next> <urn:root> \"second\" .";
const OWNER: &str = "<urn:shared> <urn:owner> \"owner\"@en, \"two\"@en; \
    <urn:link> <urn:next> . <urn:next> <urn:owner> <urn:shared> .";

fn bindings(bytes: &[u8]) -> Vec<String> {
    let value: Value = serde_json::from_slice(bytes).unwrap();
    let mut rows: Vec<_> = value["results"]["bindings"]
        .as_array()
        .unwrap()
        .iter()
        .map(ToString::to_string)
        .collect();
    rows.sort();
    rows
}

#[test]
fn enrolled_query_bags_paths_and_rdf_terms_match_single_file_baseline() {
    let (_dir, mut legacy, catalog, mut config, _emulator, _tls) = fixture();
    quipu::tool_knot(&mut legacy, &json!({"turtle": ROOT})).unwrap();
    let admission = ComposedReadAdmission::new(
        catalog.clone(),
        1024,
        3,
        1,
        ComposedReadReservation {
            runtime_bytes: 16 << 20,
            view_bytes: 16 << 20,
            query_bytes: 16 << 20,
            max_response_bytes: 1 << 20,
        },
    )
    .unwrap();
    config.allocation = admission.allocation();
    let Startup::Enabled(dataset) = PreparedDataset::initialize(
        &IngestPilot {
            enabled: true,
            graph: Some(GRAPH.into()),
        },
        catalog,
        legacy,
        &config,
        || {
            Ok((
                "AKIAIOSFODNN7EXAMPLE".into(),
                "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY".into(),
            ))
        },
        |_| Ok(()),
    )
    .unwrap() else {
        panic!("enabled startup disappeared")
    };
    // All owner readers already exist: dictionary growth must remain visible.
    let response = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(crate::auth::run_identified(
            Some(identity("named_bearer")),
            dataset
                .routes()
                .oneshot(request("/knot", &json!({"graph":GRAPH, "turtle":OWNER}))),
        ))
        .unwrap();
    assert!(response.status().is_success());

    let mut baseline = Store::open_in_memory().unwrap();
    quipu::tool_knot(&mut baseline, &json!({"turtle":ROOT})).unwrap();
    let graph = baseline.graph_create(GRAPH).unwrap();
    quipu::tool_knot(&mut baseline, &json!({"graph":GRAPH,"turtle":OWNER})).unwrap();
    let mut grants = OwnerReadPolicy::open(":memory:", "urn:principal:owner").unwrap();
    grants
        .apply(
            &ReadGrant {
                version: 1,
                id: "urn:grant:acceptance".into(),
                principal: "urn:test:startup".into(),
                graphs: [ROOT_GRAPH_IRI.into(), GRAPH.into()].into(),
                operations: [ReadOperation::Query].into(),
                valid_from: 1,
                valid_until: 100,
                max_offline_seconds: 30,
                issuer: "urn:principal:owner".into(),
                revision: 1,
                revoked: false,
            },
            "private enrolled query acceptance fixture",
        )
        .unwrap();
    let policy = grants.snapshot().unwrap();
    let who = ReadIdentity::from_verified_request(&identity("named_bearer"), 100).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    dataset.read(&[ROOT_GRAPH_IRI, GRAPH], deadline, |reads| {
        let view = policy.authorize(&who, ReadOperation::Query, None, 2)?
            .open_composed_cut(reads, admission.try_reserve()?, 2, deadline)?;
        let queries = [
            ("join", "SELECT ?s ?r ?o WHERE { ?s <urn:root> ?r . GRAPH <GRAPH> { ?s <urn:owner> ?o } }", 3),
            ("bag", "SELECT ?s ?o WHERE { { ?s <urn:link> ?o } UNION { GRAPH <GRAPH> { ?s <urn:link> ?o } } }", 2),
            ("distinct", "SELECT DISTINCT ?s ?o WHERE { { ?s <urn:link> ?o } UNION { GRAPH <GRAPH> { ?s <urn:link> ?o } } }", 1),
            ("optional", "SELECT ?s ?r ?o WHERE { ?s <urn:root> ?r OPTIONAL { GRAPH <GRAPH> { ?s <urn:owner> ?o } } }", 3),
            ("path", "SELECT ?s ?r WHERE { ?s <urn:link>/<urn:root> ?r }", 1),
            ("aggregate", "SELECT ?s (COUNT(?o) AS ?n) WHERE { GRAPH <GRAPH> { ?s <urn:owner> ?o } } GROUP BY ?s", 2),
            ("typed", "SELECT ?s ?n WHERE { ?s <urn:number> ?n }", 1),
            ("language", "SELECT ?s ?o WHERE { GRAPH <GRAPH> { ?s <urn:owner> ?o } FILTER(isLiteral(?o)) }", 2),
            ("exists", "SELECT ?s WHERE { ?s <urn:root> ?r FILTER EXISTS { GRAPH <GRAPH> { ?s <urn:owner> ?o } } }", 2),
        ];
        for (name, text, count) in queries {
            let text = text.replace("<GRAPH>", &format!("<{GRAPH}>"));
            let text = text.replacen(" WHERE", &format!(" FROM <{ROOT_GRAPH_IRI}> FROM NAMED <{GRAPH}> WHERE"), 1);
            let context = sparql::TemporalContext {
                graph: sparql::GraphScope::Default(vec![0, graph]),
                named_dataset: Some(vec![graph]), ..Default::default()
            };
            let expected = sparql::query_temporal(&baseline, &text, &context)?;
            let (_, expected) = w3c::serialize(&baseline, &expected, w3c::ResultFormat::SparqlJson)?.unwrap();
            let actual = view.query(&text, false, 2)?;
            assert_eq!(actual.content_type(), "application/sparql-results+json");
            let actual = bindings(actual.as_bytes());
            assert_eq!(actual.len(), count, "positive control {name}");
            assert_eq!(actual, bindings(&expected), "single-file parity {name}");
            view.recheck(&policy, &who, reads, 2)?;
        }
        // Authorization preflights every scope, including an unexecuted branch.
        assert!(view.query("SELECT ?s WHERE { ?s ?p ?o FILTER(false) OPTIONAL { GRAPH <urn:excluded> { ?s ?q ?v } } }", false, 2).is_err());
        Ok(())
    }).unwrap();
}
