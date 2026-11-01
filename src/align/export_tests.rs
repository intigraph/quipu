//! Tests for standard SSSOM export (aegis-blt).
//!
//! Every assertion here is about what an SSSOM consumer reads, so the fixtures
//! go through the real `propose`-shaped rows and the real `decide`, and the
//! exports are read back by an independent parser rather than by the writer's
//! own helpers.

use std::collections::BTreeMap;

use oxrdfio::{RdfFormat, RdfParser};

use super::decide::{Decision, DecisionRow, decide, decide_dated};
use super::export::{ExportFormat, ExportOptions, export, export_tsv, export_turtle};
use super::sssom::{Justification, Mapping, MappingSet, OWL_SAME_AS};

const A: &str = "https://a.example/";
const B: &str = "https://b.example/";
const CREW: &str = "https://crew.example/agent/";
const LICENSE: &str = "https://creativecommons.org/publicdomain/zero/1.0/";

fn candidate(subject: &str, object: &str, label: &str, confidence: f64) -> Mapping {
    Mapping {
        subject_id: format!("{A}{subject}"),
        subject_label: Some(label.into()),
        predicate_id: OWL_SAME_AS.into(),
        object_id: format!("{B}{object}"),
        object_label: Some(label.into()),
        mapping_justification: Justification::LexicalSimilarityThresholdMatching,
        predicate_modifier_not: None,
        confidence: Some(confidence),
        author_id: None,
        quipu_review: None,
        quipu_reviewed_by: None,
        mapping_date: None,
    }
}

/// A proposed set of four candidates, decided the way an operator does it:
/// one accepted, one negated, one declined, one left for later.
fn decided() -> MappingSet {
    let mut proposed = MappingSet::new("urn:quipu:align:aaa:bbb");
    proposed.mappings = vec![
        candidate("bobbin-release", "bobbin-release", "bobbin-release", 0.97),
        candidate("quipu", "quipu-old", "quipu", 0.91),
        candidate("tambo", "tambo-x", "tambo", 0.88),
        candidate("yupana", "yupana", "yupana", 0.86),
    ];
    proposed.sort();
    let row = |s: &str, o: &str, decision| DecisionRow {
        subject_id: format!("{A}{s}"),
        object_id: format!("{B}{o}"),
        decision,
    };
    let rows = [
        row("bobbin-release", "bobbin-release", Decision::Accept),
        row("quipu", "quipu-old", Decision::Negate),
        row("tambo", "tambo-x", Decision::Decline),
    ];
    decide_dated(&proposed, &rows, "malcolm", Some("2026-11-01"))
        .expect("decide")
        .set
}

fn options() -> ExportOptions {
    let mut prefixes = BTreeMap::new();
    prefixes.insert("A".to_string(), A.to_string());
    prefixes.insert("B".to_string(), B.to_string());
    prefixes.insert("crew".to_string(), CREW.to_string());
    ExportOptions {
        license: LICENSE.into(),
        prefixes,
        author_prefix: Some("crew".into()),
    }
}

/// An independent SSSOM/TSV reader: metadata block, then rows by column name,
/// with every CURIE expanded through the declared map plus the built-ins.
struct ReadBack {
    metadata: Vec<String>,
    curie_map: BTreeMap<String, String>,
    header: Vec<String>,
    rows: Vec<BTreeMap<String, String>>,
}

fn read_back(tsv: &str) -> ReadBack {
    let mut metadata = Vec::new();
    let mut curie_map = BTreeMap::new();
    let mut in_map = false;
    let mut lines = tsv.lines();
    let mut header = Vec::new();
    for line in lines.by_ref() {
        if let Some(meta) = line.strip_prefix('#') {
            metadata.push(meta.to_string());
            if meta == "curie_map:" {
                in_map = true;
            } else if let Some(entry) = meta.strip_prefix("  ")
                && in_map
                && !entry.starts_with(' ')
                && !entry.starts_with('-')
            {
                let (name, iri) = entry.split_once(": ").expect("curie_map entry");
                curie_map.insert(name.to_string(), iri.trim_matches('"').to_string());
            } else {
                in_map = false;
            }
            continue;
        }
        header = line.split('\t').map(str::to_string).collect();
        break;
    }
    let rows: Vec<BTreeMap<String, String>> = lines
        .filter(|l| !l.is_empty())
        .map(|l| {
            header
                .iter()
                .cloned()
                .zip(l.split('\t').map(str::to_string))
                .collect()
        })
        .collect();
    ReadBack {
        metadata,
        curie_map,
        header,
        rows,
    }
}

const BUILT_INS: [(&str, &str); 3] = [
    ("owl", "http://www.w3.org/2002/07/owl#"),
    ("semapv", "https://w3id.org/semapv/vocab/"),
    ("sssom", "https://w3id.org/sssom/"),
];

fn expand(map: &BTreeMap<String, String>, curie: &str) -> String {
    let (prefix, local) = curie.split_once(':').expect("a CURIE");
    assert!(!local.starts_with("//"), "{curie:?} is an IRI, not a CURIE");
    let base = map
        .get(prefix)
        .map(String::as_str)
        .or_else(|| {
            BUILT_INS
                .iter()
                .find(|(n, _)| *n == prefix)
                .map(|(_, i)| *i)
        })
        .unwrap_or_else(|| panic!("prefix {prefix:?} of {curie:?} is not declared"));
    format!("{base}{local}")
}

#[test]
fn tsv_export_is_standard_sssom() {
    let set = decided();
    let tsv = export_tsv(&set, &options()).expect("export");
    let back = read_back(&tsv);

    assert!(
        back.metadata
            .iter()
            .any(|m| m == &format!("license: \"{LICENSE}\""))
    );
    assert!(
        back.metadata
            .iter()
            .any(|m| m == "mapping_set_id: \"urn:quipu:align:aaa:bbb\"")
    );
    assert!(back.metadata.iter().any(|m| m == "sssom_version: \"1.1\""));
    // Built-ins are not redeclared by a canonical writer.
    for (builtin, _) in BUILT_INS {
        assert!(
            !back.curie_map.contains_key(builtin),
            "{builtin} redeclared"
        );
    }
    for required in [
        "subject_id",
        "predicate_id",
        "object_id",
        "mapping_justification",
    ] {
        assert!(
            back.header.iter().any(|h| h == required),
            "missing {required}"
        );
    }
    assert_eq!(
        back.rows.len(),
        4,
        "every row exported, declined and pending included"
    );

    // Every identifier is a CURIE whose prefix is declared or built in.
    for row in &back.rows {
        for slot in [
            "subject_id",
            "predicate_id",
            "object_id",
            "mapping_justification",
        ] {
            expand(&back.curie_map, &row[slot]);
        }
    }
    let by_subject: BTreeMap<&str, &BTreeMap<String, String>> = back
        .rows
        .iter()
        .map(|r| (r["subject_id"].as_str(), r))
        .collect();

    let accepted = by_subject["A:bobbin-release"];
    assert_eq!(accepted["predicate_id"], "owl:sameAs");
    assert_eq!(accepted["author_id"], "crew:malcolm");
    assert_eq!(accepted["mapping_date"], "2026-11-01");
    // decide records WHO and WHEN, not a new justification: the matcher's
    // reason travels unchanged, so export must not upgrade it to curation.
    assert_eq!(
        accepted["mapping_justification"],
        "semapv:LexicalSimilarityThresholdMatching"
    );
    assert_eq!(accepted["predicate_modifier"], "");

    let negated = by_subject["A:quipu"];
    assert_eq!(negated["predicate_modifier"], "Not");
    assert_eq!(negated["author_id"], "crew:malcolm");

    // Declining asserts nothing: no author, so no SSSOM reader sees curation.
    let declined = by_subject["A:tambo"];
    assert_eq!(declined["author_id"], "");
    assert_eq!(declined["quipu_review"], "declined");
    assert_eq!(declined["quipu_reviewed_by"], "crew:malcolm");
    assert_eq!(declined["mapping_date"], "2026-11-01");

    // Never ruled on: no author, no date, the matcher's own justification.
    let pending = by_subject["A:yupana"];
    assert_eq!(pending["author_id"], "");
    assert_eq!(pending["mapping_date"], "");
    assert_eq!(
        pending["mapping_justification"],
        "semapv:LexicalSimilarityThresholdMatching"
    );
    assert_eq!(pending["confidence"], "0.86");

    // The extension slots are declared, with a property in a declared prefix.
    assert!(
        back.metadata
            .iter()
            .any(|m| m == "  - slot_name: quipu_review")
    );
    assert!(
        back.metadata
            .iter()
            .any(|m| m == "    property: quipu:alignmentReview")
    );
    assert_eq!(back.curie_map["quipu"], "https://quipu.dev/ontology/");
}

#[test]
fn tsv_export_round_trips_every_asserted_field() {
    let set = decided();
    let back = read_back(&export_tsv(&set, &options()).expect("export"));
    for (original, row) in set.mappings.iter().zip(&back.rows) {
        assert_eq!(
            expand(&back.curie_map, &row["subject_id"]),
            original.subject_id
        );
        assert_eq!(
            expand(&back.curie_map, &row["object_id"]),
            original.object_id
        );
        assert_eq!(
            expand(&back.curie_map, &row["predicate_id"]),
            original.predicate_id
        );
        assert_eq!(
            row["mapping_justification"],
            original.mapping_justification.curie()
        );
        assert_eq!(row["predicate_modifier"] == "Not", original.is_negated());
        assert_eq!(
            row["confidence"].parse::<f64>().ok(),
            original.confidence,
            "confidence must survive exactly"
        );
        let author =
            (!row["author_id"].is_empty()).then(|| expand(&back.curie_map, &row["author_id"]));
        assert_eq!(
            author,
            original.author_id.as_ref().map(|a| format!("{CREW}{a}"))
        );
        assert_eq!(
            (!row["mapping_date"].is_empty()).then(|| row["mapping_date"].clone()),
            original.mapping_date
        );
    }
}

#[test]
fn export_is_deterministic_whatever_the_row_order() {
    let set = decided();
    let mut shuffled = set.clone();
    shuffled.mappings.reverse();
    for format in [ExportFormat::Tsv, ExportFormat::Turtle] {
        let first = export(&set, &options(), format).expect("export");
        assert_eq!(first, export(&set, &options(), format).expect("again"));
        assert_eq!(
            first,
            export(&shuffled, &options(), format).expect("shuffled")
        );
    }
}

fn triples(turtle: &str) -> Vec<String> {
    RdfParser::from_format(RdfFormat::Turtle)
        .for_reader(turtle.as_bytes())
        .map(|q| {
            let q = q.expect("the export parses as Turtle");
            format!("{} {} {}", q.subject, q.predicate, q.object)
        })
        .collect()
}

#[test]
fn turtle_export_is_sssom_rdf() {
    let set = decided();
    let ttl = export_turtle(&set, &options()).expect("export");
    let t = triples(&ttl);
    let count = |needle: &str| t.iter().filter(|x| x.contains(needle)).count();

    assert!(t.contains(
        &"<urn:quipu:align:aaa:bbb> <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <https://w3id.org/sssom/MappingSet>"
            .to_string()
    ));
    assert!(t.contains(&format!(
        "<urn:quipu:align:aaa:bbb> <http://purl.org/dc/terms/license> <{LICENSE}>"
    )));
    assert_eq!(count("<https://w3id.org/sssom/mappings>"), 4);
    assert_eq!(count("<http://www.w3.org/2002/07/owl#Axiom>"), 4);
    assert_eq!(count("<https://w3id.org/sssom/NegatedPredicate>"), 1);
    // Accepted and negated are authored; declined and pending are not.
    assert_eq!(
        count(&format!("<http://purl.org/pav/authoredBy> <{CREW}malcolm>")),
        2
    );
    assert_eq!(
        count(
            "<http://purl.org/dc/terms/created> \"2026-11-01\"^^<http://www.w3.org/2001/XMLSchema#date>"
        ),
        3
    );
    assert_eq!(
        count(
            "<https://w3id.org/sssom/confidence> \"0.97\"^^<http://www.w3.org/2001/XMLSchema#double>"
        ),
        1
    );
    assert_eq!(
        count(&format!(
            "<http://www.w3.org/2002/07/owl#annotatedSource> <{A}bobbin-release>"
        )),
        1
    );
    assert_eq!(count("<https://w3id.org/sssom/ExtensionDefinition>"), 2);
}

#[test]
fn an_uncovered_iri_is_refused_not_given_an_invented_prefix() {
    let mut options = options();
    options.prefixes.remove("B");
    for format in [ExportFormat::Tsv, ExportFormat::Turtle] {
        let err = export(&decided(), &options, format).expect_err("B is undeclared");
        assert!(
            err.to_string()
                .contains("not covered by any declared CURIE prefix"),
            "{err}"
        );
    }
}

#[test]
fn a_licence_is_required_and_must_be_an_iri() {
    for license in ["", "CC0", "creativecommons.org/publicdomain/zero/1.0/"] {
        let options = ExportOptions {
            license: license.into(),
            ..options()
        };
        let err = export_tsv(&decided(), &options).expect_err("no usable licence");
        assert!(err.to_string().contains("license"), "{err}");
    }
}

#[test]
fn a_bare_reviewer_needs_a_declared_author_prefix() {
    let without = ExportOptions {
        author_prefix: None,
        ..options()
    };
    let err = export_tsv(&decided(), &without).expect_err("bare name");
    assert!(err.to_string().contains("bare name"), "{err}");

    let undeclared = ExportOptions {
        author_prefix: Some("nobody".into()),
        ..options()
    };
    let err = export_tsv(&decided(), &undeclared).expect_err("undeclared prefix");
    assert!(
        err.to_string().contains("not a declared CURIE prefix"),
        "{err}"
    );

    // Control: a set with no reviewer at all needs no author prefix.
    let mut pending = MappingSet::new("urn:quipu:align:aaa:bbb");
    pending
        .mappings
        .push(candidate("yupana", "yupana", "yupana", 0.86));
    export_tsv(&pending, &without).expect("nothing to attribute");
}

#[test]
fn a_built_in_prefix_cannot_be_redefined() {
    let mut options = options();
    options
        .prefixes
        .insert("owl".into(), "https://evil.example/owl#".into());
    let err = export_tsv(&decided(), &options).expect_err("owl redefined");
    assert!(err.to_string().contains("declared as both"), "{err}");

    let mut rdf = self::options();
    rdf.prefixes
        .insert("dcterms".into(), "https://evil.example/dc/".into());
    let err = export_turtle(&decided(), &rdf).expect_err("dcterms redefined");
    assert!(err.to_string().contains("dcterms"), "{err}");
}

#[test]
fn values_sssom_cannot_carry_are_refused() {
    let mut set = decided();
    set.mappings[0].confidence = Some(1.5);
    assert!(export_tsv(&set, &options()).is_err(), "confidence above 1");

    let mut set = decided();
    set.mapping_set_id = "aaa-bbb".into();
    assert!(
        export_tsv(&set, &options()).is_err(),
        "relative mapping_set_id"
    );

    let mut set = decided();
    set.mappings[0].subject_label = Some("two\tcells".into());
    assert!(export_tsv(&set, &options()).is_err(), "tab in a label");

    let mut set = decided();
    set.mappings[0].object_id = format!("{B}has space");
    assert!(export_tsv(&set, &options()).is_err(), "space in an IRI");
}

#[test]
fn an_undated_working_set_keeps_its_bytes_and_its_version() {
    // A set decided before dates existed must serialise exactly as it did, or
    // the expected-version an operator was given stops matching.
    let mut proposed = MappingSet::new("urn:quipu:align:aaa:bbb");
    proposed
        .mappings
        .push(candidate("yupana", "yupana", "yupana", 0.86));
    let rows = [DecisionRow {
        subject_id: format!("{A}yupana"),
        object_id: format!("{B}yupana"),
        decision: Decision::Accept,
    }];
    let undated = decide(&proposed, &rows, "malcolm").expect("decide").set;
    let tsv = undated.to_tsv().expect("tsv");
    assert!(!tsv.contains("mapping_date"), "{tsv}");
    let header = tsv.lines().find(|l| !l.starts_with('#')).expect("header");
    assert_eq!(header.split('\t').count(), 11);
    assert_eq!(
        undated,
        decide_dated(&proposed, &rows, "malcolm", None)
            .expect("decide")
            .set
    );

    let dated = decide_dated(&proposed, &rows, "malcolm", Some("2026-11-01"))
        .expect("decide")
        .set;
    let text = dated.to_tsv().expect("tsv");
    assert!(
        text.lines().any(|l| l.ends_with("\tmapping_date")),
        "{text}"
    );
    assert_eq!(MappingSet::from_tsv(&text).expect("parse"), dated);
}

#[test]
fn decide_dates_only_the_rows_it_rules_on() {
    let set = decided();
    let dates: BTreeMap<&str, Option<&str>> = set
        .mappings
        .iter()
        .map(|m| (m.subject_id.as_str(), m.mapping_date.as_deref()))
        .collect();
    assert_eq!(
        dates[format!("{A}bobbin-release").as_str()],
        Some("2026-11-01")
    );
    assert_eq!(dates[format!("{A}tambo").as_str()], Some("2026-11-01"));
    assert_eq!(dates[format!("{A}yupana").as_str()], None);
}

#[test]
fn a_malformed_date_is_refused_everywhere() {
    let mut proposed = MappingSet::new("urn:quipu:align:aaa:bbb");
    proposed
        .mappings
        .push(candidate("yupana", "yupana", "yupana", 0.86));
    for bad in [
        "2026-13-01",
        "2026-11-32",
        "26-11-01",
        "2026-11-01T00:00:00Z",
        "",
    ] {
        assert!(
            decide_dated(&proposed, &[], "malcolm", Some(bad)).is_err(),
            "decide accepted {bad:?}"
        );
    }
    let mut set = decided();
    set.mappings[0].mapping_date = Some("2026-13-01".into());
    assert!(set.to_tsv().is_err(), "to_tsv wrote a bad date");
    assert!(
        export_tsv(&set, &options()).is_err(),
        "export wrote a bad date"
    );

    let good = decided().to_tsv().expect("tsv");
    let sabotaged = good.replacen("2026-11-01", "2026-11-99", 1);
    assert_ne!(good, sabotaged, "control: the sabotage changed the text");
    assert!(
        MappingSet::from_tsv(&sabotaged).is_err(),
        "from_tsv read a bad date"
    );
}
