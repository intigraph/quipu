use super::parse_rules;

fn rules(head: &str, body: &str) -> String {
    format!(
        "@prefix rule: <http://quipu.local/rule#> .\n\
         <urn:test:rule> a rule:Rule; rule:head \"{head}\"; rule:body \"{body}\" ."
    )
}

#[test]
fn unqualified_names_require_an_explicit_prefix() {
    for (head, body) in [
        ("result(?x)", "<urn:test:input>(?x)"),
        ("<urn:test:result>(?x)", "input(?x)"),
        ("<urn:test:result>(named)", "<urn:test:input>(named)"),
    ] {
        for prefix in [None, Some("")] {
            let error = parse_rules(&rules(head, body), prefix)
                .expect_err("an omitted prefix must never guess an ontology");
            assert!(error.to_string().contains("prefix"), "{error}");
        }
    }
}

#[test]
fn fully_qualified_rules_need_no_ambient_namespace() {
    let parsed = parse_rules(
        &rules("<urn:test:result>(?x)", "<urn:test:input>(?x)"),
        None,
    )
    .unwrap();
    assert!(parsed.default_prefix.is_empty());
    assert_eq!(parsed.rules[0].head.predicate, "urn:test:result");
}

#[test]
fn explicit_store_prefix_resolves_predicates_and_named_terms() {
    let store_prefix = "https://example.org/custom#";
    let parsed = parse_rules(&rules("result(named)", "input(named)"), Some(store_prefix))
        .unwrap();
    assert_eq!(parsed.rules[0].head.predicate, format!("{store_prefix}result"));
    assert_eq!(
        parsed.rules[0].head.args[0],
        super::Term::Iri(format!("{store_prefix}named"))
    );
}
