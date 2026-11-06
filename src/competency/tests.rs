use super::*;

const VALID: &str = r#"
schema = "quipu-competency/1"
[[question]]
id = "people"
ask = "Who is present?"
query = "SELECT ?person WHERE { ?person a <urn:Person> }"
returns = {person = "iri"}
justifies = ["urn:PersonShape"]
expect = "nonempty"
[[question]]
id = "present"
ask = "Is anyone present?"
query = "ASK { ?p a <urn:Person> }"
justifies = ["urn:Person"]
expect = "ask=true"
[[question]]
id = "projection"
ask = "What are the names?"
query = "CONSTRUCT { ?p <urn:name> ?n } WHERE { ?p <urn:name> ?n }"
returns_shape = "urn:NameShape"
justifies = ["urn:NameShape"]
[[command]]
id = "record"
does = "Record a person"
inputs = { person = "iri", name = "string" }
template = '<{person}> a <urn:Person> ; <urn:name> "{name}" .'
satisfies = ["urn:PersonShape"]
write_class = "arbitrary-user-class"
key = ["person"]
example = { person = "urn:alice", name = "Alice" }
"#;

#[test]
fn valid_contract_compiles_to_parseable_deterministic_rdf() {
    let c = parse(VALID, &[]).unwrap();
    let a = c.compile(&[]).unwrap();
    let mut reordered = c.clone();
    reordered.questions.reverse();
    assert_eq!(a, reordered.compile(&[]).unwrap());
    for ttl in [&a.queries, &a.commands] {
        let parsed = oxrdfio::RdfParser::from_format(oxrdfio::RdfFormat::Turtle)
            .for_reader(ttl.as_bytes())
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap();
        assert!(!parsed.is_empty());
    }
    assert!(a.queries.contains("quipu:StoredQuery"));
    assert!(a.queries.contains("quipu:justifies <urn:PersonShape>"));
    assert!(a.queries.contains("quipu:returns"));
    assert!(a.commands.contains("arbitrary-user-class"));
    assert!(a.commands.contains("quipu:key \"person\""));
    assert_eq!(
        a,
        parse(&toml::to_string(&c).unwrap(), &[])
            .unwrap()
            .compile(&[])
            .unwrap()
    );
}

fn rejects(text: &str, needle: &str) {
    let error = parse(text, &[]).unwrap_err().to_string();
    assert!(error.contains(needle), "{error} did not contain {needle}");
}

#[test]
fn refuses_unknown_schema_blocks_fields_and_duplicate_identity() {
    rejects(
        &VALID.replace("quipu-competency/1", "quipu-competency/2"),
        "schema",
    );
    rejects(&format!("{VALID}\n[[fluent]]\nid='x'"), "unknown field");
    rejects(&VALID.replace("does =", "doze ="), "unknown field");
    rejects(
        &VALID.replace("id = \"record\"", "id = \"people\""),
        "duplicate id",
    );
    rejects(
        &VALID.replace("id = \"people\"", "id = \"../escape\""),
        "invalid or duplicate id",
    );
}

#[test]
fn refuses_broken_queries_and_output_contracts_by_id() {
    rejects(
        &VALID.replace("SELECT ?person WHERE", "BROKEN ?person WHERE"),
        "question people",
    );
    rejects(
        &VALID.replace("person = \"iri\"}", "other = \"iri\"}"),
        "projected columns",
    );
    rejects(
        &VALID.replace("expect = \"ask=true\"", "expect = \"nonempty\""),
        "question present",
    );
    rejects(
        &VALID.replace(
            "returns_shape = \"urn:NameShape\"",
            "returns_shape = \"not an iri\"",
        ),
        "question projection",
    );
    rejects(
        &VALID.replace("justifies = [\"urn:PersonShape\"]", "justifies = []"),
        "question people",
    );
    rejects(
        &VALID.replace(
            "query = \"ASK { ?p a <urn:Person> }\"",
            "query = \"DELETE WHERE { ?s ?p ?o }\"",
        ),
        "question present",
    );
}

#[test]
fn refuses_command_syntax_undeclared_inputs_keys_and_noninsert_updates() {
    rejects(
        &VALID.replace(
            "template = '<{person}> a <urn:Person> ; <urn:name> \"{name}\" .'",
            "template = 'not turtle'",
        ),
        "command record",
    );
    rejects(
        &VALID.replace("{name}\" .", "{typo}\" ."),
        "undeclared placeholder",
    );
    rejects(
        &VALID.replace("key = [\"person\"]", "key = [\"absent\"]"),
        "command record",
    );
    rejects(
        &VALID.replace("name = \"string\"", "name = \"widget\""),
        "unsupported scalar",
    );
    rejects(
        &VALID.replace("person = \"urn:alice\", ", ""),
        "example missing required",
    );
    let mut c = parse(VALID, &[]).unwrap();
    c.commands[0].template = "DELETE WHERE { <{person}> <urn:name> '{name}' }".into();
    assert!(
        c.lint(&[])
            .unwrap_err()
            .to_string()
            .contains("only nonempty INSERT DATA")
    );
    c.commands[0].template = "INSERT DATA { <{person}> <urn:name> '{name}' }".into();
    c.lint(&[]).unwrap();
}

#[test]
fn optional_inputs_are_not_assigned_execution_defaults() {
    let c = parse(
        &VALID.replace("name = \"string\"", "name = \"string?\""),
        &[],
    )
    .unwrap();
    assert!(c.compile(&[]).unwrap().commands.contains("string?"));
}

#[test]
fn reference_requires_one_offline_definition() {
    let mut c = parse(VALID, &[]).unwrap();
    c.questions[0].query = None;
    c.questions[0].query_ref = Some("source".into());
    assert!(c.lint(&[]).unwrap_err().to_string().contains("query_ref"));
    let registry = vec![
        crate::share_queries::inspect(crate::store::queries::StoredQuery {
            name: "source".into(),
            description: String::new(),
            template: "SELECT ?person WHERE { ?person a <urn:Person> }".into(),
            dataset: None,
            params: vec![],
        })
        .unwrap(),
    ];
    c.lint(&registry).unwrap();
    let compiled = c.compile(&registry).unwrap();
    assert!(compiled.queries.contains("quipu:queryRef \"source\""));
    assert!(c.lint(&[registry[0].clone(), registry[0].clone()]).is_err());
    c.questions[0].query = Some("ASK {}".into());
    assert!(
        c.lint(&registry)
            .unwrap_err()
            .to_string()
            .contains("exactly one")
    );
}
