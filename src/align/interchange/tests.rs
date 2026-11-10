use super::*;

fn input(extra: &str, columns: &str, row: &str) -> String {
    format!(
        "#mapping_set_id: urn:example:set\n#curie_map:\n#  ex: https://example.org/\n{extra}subject_id\tpredicate_id\tobject_id\tmapping_justification{columns}\nex:a\tskos:exactMatch\tex:b\tsemapv:LexicalMatching{row}\n"
    )
}

#[test]
fn preserves_external_evidence_and_unknown_justification() {
    let text = input(
        "#custom: [one, two]\n",
        "\tauthor_id\taccepted",
        "\tex:attacker\ttrue",
    )
    .replace("semapv:LexicalMatching", "ex:NewMatcher");
    let set = ExternalSet::parse(&text).unwrap();
    assert_eq!(set.source, text);
    assert_eq!(set.rows[0].justification, "https://example.org/NewMatcher");
    assert_eq!(set.rows[0].fields["accepted"], "true");
    assert_eq!(set.metadata["custom"], serde_json::json!(["one", "two"]));
    assert!(set.rows[0].identity_eligible()); // eligible is NOT accepted
}

#[test]
fn propagation_and_reordered_columns_preserve_operational_terms() {
    let a = input(
        "#mapping_justification: semapv:LexicalMatching\n#confidence: 1\n",
        "",
        "",
    )
    .replace("\tmapping_justification", "")
    .replace("\tsemapv:LexicalMatching", "");
    let set = ExternalSet::parse(&a).unwrap();
    assert_eq!(set.rows[0].fields["confidence"], "1");
    assert_eq!(set.rows[0].subject, "https://example.org/a");
    let b = a
        .replace(
            "subject_id\tpredicate_id\tobject_id",
            "object_id\tpredicate_id\tsubject_id",
        )
        .replace("ex:a\tskos:exactMatch\tex:b", "ex:b\tskos:exactMatch\tex:a");
    let other = ExternalSet::parse(&b).unwrap();
    assert_eq!(set.rows, other.rows);
    assert_ne!(set.source_sha256, other.source_sha256);
}

#[test]
fn negative_declined_and_nonexact_rows_never_qualify_as_identity() {
    for (columns, row) in [
        ("\tpredicate_modifier", "\tNot"),
        ("\tquipu_review", "\tdeclined"),
    ] {
        assert!(!ExternalSet::parse(&input("", columns, row)).unwrap().rows[0].identity_eligible());
    }
    for p in [
        "skos:broadMatch",
        "skos:narrowMatch",
        "skos:closeMatch",
        "ex:unknown",
    ] {
        let text = input("", "", "").replace("skos:exactMatch", p);
        assert!(!ExternalSet::parse(&text).unwrap().rows[0].identity_eligible());
    }
}

#[test]
fn ambiguous_metadata_and_alias_expansion_refuse() {
    for header in [
        "#mapping_set_id: urn:another\n",
        "#curie_map: {ex: 'https://other.example/'}\n",
        "#custom: &x [one]\n#copy: *x\n",
        "#---\n#mapping_set_id: urn:another\n",
        "#curie_map: invalid\n",
    ] {
        assert!(
            ExternalSet::parse(&input(header, "", "")).is_err(),
            "{header}"
        );
    }
}

#[test]
fn malformed_width_duplicate_columns_and_invalid_operational_fields_refuse() {
    let base = input("", "", "");
    for text in [
        base.replace("object_id", "subject_id"),
        base.replace("ex:b\t", "ex:b\textra\t"),
        base.replace("ex:a", "missing:a"),
        base.replace("ex:a", "a"),
        base.replace("ex:a", " ex:a"),
        base.replace("ex:a", "ex:a|ex:c"),
        base.replace("ex:b", "ex:bad space"),
        base.replace("urn:example:set", ""),
    ] {
        assert!(ExternalSet::parse(&text).is_err(), "{text}");
    }
    for (column, value) in [
        ("predicate_modifier", "not"),
        ("quipu_review", "accepted"),
        ("confidence", "NaN"),
        ("confidence", "inf"),
        ("confidence", "-0.1"),
        ("confidence", "1.1"),
        ("mapping_date", "2026-02-30"),
    ] {
        assert!(
            ExternalSet::parse(&input("", &format!("\t{column}"), &format!("\t{value}"))).is_err()
        );
    }
}

#[test]
fn declared_scheme_and_builtin_prefix_redefinition_refuse() {
    for (prefix, iri) in [
        ("skos", "https://evil.example/"),
        ("http", "https://example.org/"),
        ("urn", "urn:ex:"),
    ] {
        let text = input("", "", "").replace(
            "#  ex: https://example.org/",
            &format!("#  ex: https://example.org/\n#  {prefix}: {iri}"),
        );
        assert!(ExternalSet::parse(&text).is_err(), "{prefix}");
    }
}

#[test]
fn row_identity_retains_duplicates_and_detects_changed_extra_evidence() {
    let text = input("", "\tcomment", "\tfirst");
    let row = text.lines().last().unwrap();
    let set = ExternalSet::parse(&format!("{text}{row}\n")).unwrap();
    assert_eq!(set.rows.len(), 2);
    assert_eq!(set.rows[0].row_sha256, set.rows[1].row_sha256);
    assert_ne!(set.rows[0].ordinal, set.rows[1].ordinal);
    let other = ExternalSet::parse(&text.replace("first", "second")).unwrap();
    assert_ne!(set.rows[0].row_sha256, other.rows[0].row_sha256);
}

#[test]
fn bounds_and_empty_standard_set() {
    let text = input("", "", "");
    let empty = text
        .lines()
        .take(text.lines().count() - 1)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(ExternalSet::parse(&empty).unwrap().rows.is_empty());
    assert!(ExternalSet::parse(&"x".repeat(MAX_BYTES + 1)).is_err());
    let big_meta = input(&format!("#custom: {}\n", "x".repeat(65_536)), "", "");
    assert!(ExternalSet::parse(&big_meta).is_err());
    let row = text.lines().last().unwrap();
    let over_rows = format!("{empty}\n{}", format!("{row}\n").repeat(MAX_ROWS + 1));
    assert!(ExternalSet::parse(&over_rows).is_err());
}

proptest::proptest! {
    #[test]
    fn original_evidence_is_never_discarded(comment in "[a-zA-Z0-9 _-]{0,80}") {
        let text = input("", "\tcomment", &format!("\t{comment}"));
        let set = ExternalSet::parse(&text).unwrap();
        proptest::prop_assert_eq!(&set.source, &text);
        proptest::prop_assert_eq!(&set.rows[0].fields["comment"], &comment);
        let other = ExternalSet::parse(&format!("{text}\n")).unwrap();
        proptest::prop_assert_eq!(&set.rows, &other.rows);
        proptest::prop_assert_ne!(&set.source_sha256, &other.source_sha256);
    }
}

#[test]
fn serialized_derived_fields_and_metadata_amplification_cannot_bypass_reader() {
    let text = input("", "", "");
    let mut set = ExternalSet::parse(&text).unwrap();
    set.rows[0].predicate = "http://www.w3.org/2002/07/owl#sameAs".into();
    assert!(set.verify_source().is_err());
    let header = format!("#custom: {}\n", "x".repeat(60_000));
    let text = input(&header, "", "");
    let row = text.lines().last().unwrap();
    let expanded = format!("{text}{}", format!("{row}\n").repeat(600));
    assert!(expanded.len() < MAX_BYTES);
    assert!(
        ExternalSet::parse(&expanded)
            .unwrap_err()
            .to_string()
            .contains("expanded rows")
    );
}
