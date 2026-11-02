use super::*;

fn row(source: &str, target: &str, predicate: &str) -> Correspondence {
    Correspondence {
        subject_id: source.into(),
        object_id: target.into(),
        predicate_id: predicate.into(),
        mapping_justification: "semapv:ManualMappingCuration".into(),
        author_id: "urn:reviewer".into(),
        quipu_review: "accepted".into(),
    }
}
fn mapping() -> ShapeMapping {
    ShapeMapping {
        id: "urn:mapping:one".into(),
        graph: "urn:mappings".into(),
        maps_from: ["urn:source".into()].into(),
        maps_to: ["urn:target".into()].into(),
        pins: [ShapePin {
            registry: RegistryKind::Shapes,
            name: "fixture".into(),
            valid_from: "2026-10-25T00:00:00Z".into(),
            digest: canonical_shape_digest("<urn:s> <urn:p> [] .").unwrap(),
        }]
        .into(),
        rows: [row("urn:their:Ticket", "urn:my:Task", EQUIVALENT_CLASS)].into(),
    }
}
#[test]
fn canonical_rdf_digest_ignores_spelling_and_tracks_semantics() {
    let first = canonical_shape_digest("@prefix x: <urn:> . x:s x:p [x:q x:v] .").unwrap();
    assert_eq!(
        first,
        canonical_shape_digest("_:other <urn:q> <urn:v> . <urn:s> <urn:p> _:other .").unwrap()
    );
    assert_ne!(
        first,
        canonical_shape_digest("<urn:s> <urn:p> [<urn:q> <urn:different>] .").unwrap()
    );
    assert!(canonical_shape_digest("broken turtle").is_err());
}
#[test]
fn inverse_uniqueness_is_separate_from_conflicts_and_one_way_rows() {
    let mut m = mapping();
    assert!(m.assert_writable().unwrap()["urn:my:Task"]);
    let old = m.digest().unwrap();
    m.rows
        .insert(row("urn:their:Issue", "urn:my:Task", EQUIVALENT_CLASS));
    assert!(!m.assert_writable().unwrap()["urn:my:Task"]);
    assert_ne!(old, m.digest().unwrap());
    m.rows = [row("urn:their:Ticket", "urn:my:Task", SUBCLASS)].into();
    assert!(!m.assert_writable().unwrap()["urn:my:Task"]);
    m.rows = [row("urn:their:p", "urn:my:q", INVERSE)].into();
    assert!(m.assert_writable().unwrap()["urn:my:q"]);
}
#[test]
fn chain_conflict_review_and_predicate_refusals_have_positive_controls() {
    let m = mapping();
    let mut other = mapping();
    other.id = "urn:mapping:two".into();
    other.rows = [row("urn:my:Task", "urn:third:Work", EQUIVALENT_CLASS)].into();
    assert!(m.check_other(&other).is_err());
    other.rows = [row(
        "urn:their:Ticket",
        "urn:my:Different",
        EQUIVALENT_CLASS,
    )]
    .into();
    assert!(m.check_other(&other).is_err());
    other.maps_from = ["urn:unrelated:a".into()].into();
    other.maps_to = ["urn:unrelated:b".into()].into();
    assert!(m.check_other(&other).is_ok());
    let mut bad = mapping();
    bad.rows = [row(
        "urn:their:Ticket",
        "urn:my:Task",
        "http://www.w3.org/2002/07/owl#sameAs",
    )]
    .into();
    assert!(bad.validate(true).is_err());
    let mut bad = mapping();
    let mut unreviewed = bad.rows.pop_first().unwrap();
    unreviewed.quipu_review.clear();
    bad.rows.insert(unreviewed);
    assert!(bad.validate(false).is_ok());
    assert!(bad.validate(true).is_err());
    assert!(m.validate(true).is_ok());
}
#[test]
fn digest_migration_preserves_history_and_marks_legacy_unknown() {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    for table in ["shapes", "ontologies"] {
        conn.execute_batch(&format!("CREATE TABLE {table}(name TEXT,turtle TEXT,valid_from TEXT,valid_to TEXT,tx INTEGER); INSERT INTO {table} VALUES('one','<urn:s> <urn:p> <urn:o> .','before','after',7); INSERT INTO {table} VALUES('legacy','not RDF','old',NULL,8);")).unwrap();
    }
    migrate_registry_digests(&conn).unwrap();
    migrate_registry_digests(&conn).unwrap();
    let x: (String, String, i64, String) = conn
        .query_row(
            "SELECT valid_from,valid_to,tx,digest FROM shapes WHERE name='one'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap();
    assert_eq!((x.0.as_str(), x.1.as_str(), x.2), ("before", "after", 7));
    assert_eq!(
        x.3,
        canonical_shape_digest("<urn:s> <urn:p> <urn:o> .").unwrap()
    );
    let unknown: Option<String> = conn
        .query_row("SELECT digest FROM shapes WHERE name='legacy'", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert!(unknown.is_none());
}

#[test]
fn explicit_decision_binds_whole_rows_and_rejects_partial_or_foreign_input() {
    use super::catalog::{MappingProposal, RowDecision};
    let mut m = mapping();
    m.rows
        .insert(row("urn:their:Other", "urn:my:Other", EQUIVALENT_CLASS));
    let decisions: Vec<_> = m
        .rows
        .iter()
        .enumerate()
        .map(|(index, r)| RowDecision {
            subject_id: r.subject_id.clone(),
            predicate_id: r.predicate_id.clone(),
            object_id: r.object_id.clone(),
            accept: index == 0,
        })
        .collect();
    let proposal = MappingProposal {
        expected_digest: m.digest().unwrap(),
        mapping: m,
    };
    assert!(proposal.clone().decide("urn:reviewer", vec![]).is_err());
    let mut stale = proposal.clone();
    stale.mapping.maps_to.insert("urn:another:target".into());
    assert!(stale.decide("urn:reviewer", decisions.clone()).is_err());
    let mut duplicate = decisions.clone();
    duplicate.push(decisions[0].clone());
    assert!(proposal.clone().decide("urn:reviewer", duplicate).is_err());
    let mut foreign = decisions.clone();
    foreign.push(RowDecision {
        subject_id: "urn:foreign".into(),
        predicate_id: EQUIVALENT_CLASS.into(),
        object_id: "urn:foreign:target".into(),
        accept: true,
    });
    assert!(proposal.clone().decide("urn:reviewer", foreign).is_err());
    let result = proposal.decide("urn:reviewer:actual", decisions).unwrap();
    assert_eq!(result.mapping.rows.len(), 1);
    assert_eq!(result.declined_rows.len(), 1);
    assert_eq!(result.expected_digest, result.mapping.digest().unwrap());
    assert!(
        result
            .mapping
            .rows
            .iter()
            .all(|r| r.author_id == "urn:reviewer:actual" && r.quipu_review == "accepted")
    );
}

#[test]
fn mapping_rdf_is_inert_and_record_binds_actual_transaction_and_exact_cause() {
    use super::facts::{MappingStatus, Settlement, definition, record};
    let m = mapping();
    let at = Settlement {
        actor: "urn:actor:actual",
        authority: "urn:decision:actual",
        timestamp: "2026-10-25T00:00:00Z",
        tx: 42,
    };
    let rdf = definition(&m, &at).unwrap();
    let parsed = oxrdfio::RdfParser::from_format(oxrdfio::RdfFormat::NTriples)
        .for_reader(rdf.as_bytes())
        .collect::<std::result::Result<Vec<_>, _>>()
        .unwrap();
    // A declaration must never activate OWL/RDFS equivalence in the store.
    assert!(
        !parsed
            .iter()
            .any(|q| q.predicate.as_str() == EQUIVALENT_CLASS)
    );
    assert!(parsed.iter().any(
        |q| q.predicate.as_str() == "https://w3id.org/sssom/predicate_id"
            && q.object.to_string() == format!("<{EQUIVALENT_CLASS}>")
    ));
    let (id, audit) = record(&m, &at, None, MappingStatus::Decided, None, None).unwrap();
    assert!(id.ends_with(":42"));
    assert!(audit.contains("<urn:decision:actual>"));
    assert!(audit.contains("<urn:actor:actual>"));
    assert!(
        record(
            &m,
            &at,
            Some(MappingStatus::Decided),
            MappingStatus::Flagged,
            Some(&m.digest().unwrap()),
            None
        )
        .is_err()
    );
    let pin = m.pins.first().unwrap();
    let (_, audit) = record(
        &m,
        &at,
        Some(MappingStatus::Decided),
        MappingStatus::Flagged,
        Some(&m.digest().unwrap()),
        Some(pin),
    )
    .unwrap();
    assert!(audit.contains("causedByShapeVersion"));
    let mut foreign = pin.clone();
    foreign.name = "foreign".into();
    assert!(
        record(
            &m,
            &at,
            Some(MappingStatus::Decided),
            MappingStatus::Flagged,
            Some(&m.digest().unwrap()),
            Some(&foreign)
        )
        .is_err()
    );
    let invalid = Settlement { tx: 0, ..at };
    assert!(definition(&m, &invalid).is_err());
}
