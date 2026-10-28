use super::*;

fn st(triple: &str) -> Statement {
    Statement::new(triple, None)
}

fn sample() -> ChangeSet {
    ChangeSet::new(
        Header {
            base_tx: Some(4180),
            target_tx: Some(4182),
            source: Some("fork:feature".into()),
            shapes_hash: Some("abc".into()),
            ..Header::default()
        },
        vec![
            st("<http://ex/s> <http://ex/status> \"open\""),
            Statement::new(
                "<http://ex/s> <http://ex/owner> \"wu\"",
                Some("http://ex/g".into()),
            ),
        ],
        vec![st("<http://ex/s> <http://ex/status> \"closed\"")],
    )
    .unwrap()
}

#[test]
fn sparql_form_is_header_then_delete_then_insert() {
    let text = sample().to_sparql_update();
    assert!(text.starts_with("# quipu-changeset: v1\n# base-tx: 4180\n# target-tx: 4182\n"));
    let del = text.find("DELETE DATA {").unwrap();
    let ins = text.find("INSERT DATA {").unwrap();
    assert!(del < ins, "{text}");
    assert!(
        text.contains("  GRAPH <http://ex/g> { <http://ex/s> <http://ex/owner> \"wu\" . }\n"),
        "{text}"
    );
    // A plain SPARQL 1.1 parser accepts it: the header is a comment.
    spargebra::SparqlParser::new()
        .parse_update(&text)
        .expect("valid SPARQL Update");
}

#[test]
fn both_forms_parse_back_to_the_same_changeset_and_digest() {
    let cs = sample();
    let from_sparql = ChangeSet::parse(&cs.to_sparql_update(), SPARQL_UPDATE).unwrap();
    let from_patch = ChangeSet::parse(&cs.to_rdf_patch(), RDF_PATCH).unwrap();
    assert_eq!(from_sparql, cs);
    assert_eq!(from_patch, cs);
    assert_eq!(from_patch.digest(), cs.digest());
}

#[test]
fn statements_are_sorted_and_deduplicated_so_bytes_are_stable() {
    let a = ChangeSet::new(
        Header::default(),
        vec![],
        vec![
            st("<urn:b> <urn:p> \"2\""),
            st("<urn:a> <urn:p> \"1\""),
            st("<urn:a> <urn:p> \"1\""),
        ],
    )
    .unwrap();
    let b = ChangeSet::new(
        Header::default(),
        vec![],
        vec![st("<urn:a> <urn:p> \"1\""), st("<urn:b> <urn:p> \"2\"")],
    )
    .unwrap();
    assert_eq!(a.to_sparql_update(), b.to_sparql_update());
    assert_eq!(a.inserts.len(), 2);
}

#[test]
fn digest_covers_statements_not_header() {
    let mut other = sample();
    other.header.source = Some("elsewhere".into());
    assert_eq!(other.digest(), sample().digest());
    let changed = ChangeSet::new(
        Header::default(),
        vec![],
        vec![st("<urn:a> <urn:p> <urn:o>")],
    )
    .unwrap();
    assert_ne!(changed.digest(), sample().digest());
}

#[test]
fn a_declared_digest_must_match() {
    let text = sample()
        .to_sparql_update()
        .replace("# digest: sha256:", "# digest: sha256:00");
    let err = ChangeSet::parse(&text, SPARQL_UPDATE).unwrap_err();
    assert!(err.to_string().contains("digest"), "{err}");
}

#[test]
fn unrestricted_updates_are_refused() {
    for text in [
        "DELETE { ?s ?p ?o } WHERE { ?s ?p ?o }",
        "CLEAR ALL",
        "INSERT { <urn:a> <urn:p> ?o } WHERE { <urn:b> <urn:p> ?o }",
    ] {
        assert!(ChangeSet::parse(text, SPARQL_UPDATE).is_err(), "{text}");
    }
}

#[test]
fn a_statement_both_deleted_and_inserted_is_refused() {
    let text = "DELETE DATA { <urn:a> <urn:p> \"x\" } ; INSERT DATA { <urn:a> <urn:p> \"x\" }";
    let err = ChangeSet::parse(text, SPARQL_UPDATE).unwrap_err();
    assert!(err.to_string().contains("both"), "{err}");
}

#[test]
fn a_headerless_share_delta_still_parses() {
    let text = "DELETE DATA {\n  <urn:s> <urn:p> \"a\" .\n};\nINSERT DATA {\n  <urn:s> <urn:p> \"b\" .\n};\n";
    let cs = ChangeSet::parse(text, SPARQL_UPDATE).unwrap();
    assert_eq!(cs.deletes, vec![st("<urn:s> <urn:p> \"a\"")]);
    assert_eq!(cs.header, Header::default());
    // And the body this format renders is byte-identical to the share-delta body.
    assert!(cs.to_sparql_update().ends_with(text));
}

#[test]
fn unknown_header_keys_are_ignored_and_known_ones_round_trip() {
    let text = "# quipu-changeset: v1\n# future-key: x\n# valid-time: 2026-10-01T00:00:00Z\n\
                # store-id: s1\n# base-share: sha256:p\nINSERT DATA { <urn:a> <urn:p> 1 }";
    let cs = ChangeSet::parse(text, SPARQL_UPDATE).unwrap();
    assert_eq!(
        cs.header.valid_time.as_deref(),
        Some("2026-10-01T00:00:00Z")
    );
    assert_eq!(cs.header.store_id.as_deref(), Some("s1"));
    assert_eq!(cs.header.base_share.as_deref(), Some("sha256:p"));
    let again = ChangeSet::parse(&cs.to_rdf_patch(), RDF_PATCH).unwrap();
    assert_eq!(again.header, cs.header);
}

#[test]
fn an_unknown_version_is_refused() {
    let err = ChangeSet::parse("# quipu-changeset: v9\n", SPARQL_UPDATE).unwrap_err();
    assert!(err.to_string().contains("v9"), "{err}");
}

#[test]
fn rdf_patch_rejects_rows_it_does_not_understand() {
    for text in ["X <urn:a> <urn:p> <urn:o> .", "PA \"ex\" <http://ex/> ."] {
        assert!(ChangeSet::parse(text, RDF_PATCH).is_err(), "{text}");
    }
}

#[test]
fn negotiation_prefers_sparql_and_honours_rdf_patch() {
    assert_eq!(negotiate(None), SPARQL_UPDATE);
    assert_eq!(negotiate(Some("*/*")), SPARQL_UPDATE);
    assert_eq!(negotiate(Some("application/rdf-patch")), RDF_PATCH);
    assert_eq!(
        negotiate(Some("text/html, application/rdf-patch;q=0.9")),
        RDF_PATCH
    );
    assert_eq!(negotiate(Some("application/sparql-update")), SPARQL_UPDATE);
}

#[test]
fn from_states_is_the_set_difference() {
    let before = [st("<urn:a> <urn:p> \"1\""), st("<urn:a> <urn:q> \"k\"")];
    let after = [st("<urn:a> <urn:p> \"2\""), st("<urn:a> <urn:q> \"k\"")];
    let cs = ChangeSet::from_states(Header::default(), before, after);
    assert_eq!(cs.deletes, vec![st("<urn:a> <urn:p> \"1\"")]);
    assert_eq!(cs.inserts, vec![st("<urn:a> <urn:p> \"2\"")]);
}

/// A producer that follows the spec by hand, not through this module, must be
/// accepted: one `sha256:` prefix on the wire, bare hex from `digest()`.
#[test]
fn a_hand_built_spec_conformant_changeset_is_accepted() {
    let body_lines = "D <urn:s> <urn:p> \"a\" .\nA <urn:s> <urn:p> \"b\" .\n";
    let hex = crate::share::sha256(body_lines.as_bytes());
    let hex = hex.strip_prefix("sha256:").unwrap();
    assert_eq!(hex.len(), 64);
    let text = format!(
        "# quipu-changeset: v1\n# digest: sha256:{hex}\n\
         DELETE DATA {{\n  <urn:s> <urn:p> \"a\" .\n}};\nINSERT DATA {{\n  <urn:s> <urn:p> \"b\" .\n}};\n"
    );
    let cs = ChangeSet::parse(&text, SPARQL_UPDATE).unwrap();
    assert_eq!(cs.digest(), hex);
    // And this module writes exactly those bytes back.
    assert_eq!(cs.to_sparql_update(), text);
    let patch = format!(
        "H quipu-changeset \"v1\" .\nH digest \"sha256:{hex}\" .\nTX .\n{body_lines}TC .\n"
    );
    assert_eq!(cs.to_rdf_patch(), patch);
    assert_eq!(ChangeSet::parse(&patch, RDF_PATCH).unwrap(), cs);
    // A double prefix is a mismatch, not a second accepted spelling.
    let doubled = text.replace("sha256:", "sha256:sha256:");
    assert!(ChangeSet::parse(&doubled, SPARQL_UPDATE).is_err());
}

#[test]
fn valid_time_must_be_a_timestamp_and_is_normalized() {
    let err = ChangeSet::parse(
        "# quipu-changeset: v1\n# valid-time: not-a-time\n",
        SPARQL_UPDATE,
    )
    .unwrap_err();
    assert!(err.to_string().contains("valid-time"), "{err}");
    let cs = ChangeSet::parse(
        "# quipu-changeset: v1\n# valid-time: 2026-10-01T02:00:00+02:00\n",
        SPARQL_UPDATE,
    )
    .unwrap();
    assert_eq!(
        cs.header.valid_time.as_deref(),
        Some("2026-10-01T00:00:00Z")
    );
}
