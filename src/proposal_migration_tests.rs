use super::*;
use crate::proposal::{NewProposal, ProposalKind, ProposalStatus};
use crate::sparql::{TemporalContext, query, query_temporal};
use crate::types::Value;

const EX: &str = "http://example.org/";
const T0: &str = "2026-01-01T00:00:00Z";
const BEFORE: &str = "2026-03-01T00:00:00Z";
const DECIDED: &str = "2026-06-01T00:00:00Z";

fn ex(local: &str) -> String {
    format!("{EX}{local}")
}

fn store_with(turtle: &str) -> Store {
    let mut store = Store::open_in_memory().unwrap();
    crate::rdf::ingest_rdf(
        &mut store,
        turtle.as_bytes(),
        oxrdfio::RdfFormat::Turtle,
        None,
        T0,
        Some("test"),
        Some("seed"),
    )
    .unwrap();
    store
}

const PEOPLE: &str = r#"
@prefix ex: <http://example.org/> .
ex:alice a ex:Person ; ex:name "Alice Smith" ; ex:age "42" .
ex:bob a ex:Person ; ex:name "Bob Jones" ; ex:age "7" .
"#;

const PREFIXES: &str = "@prefix sh: <http://www.w3.org/ns/shacl#> .\n\
     @prefix ex: <http://example.org/> .\n\
     @prefix xsd: <http://www.w3.org/2001/XMLSchema#> .\n";

fn shape(body: &str) -> String {
    format!("{PREFIXES}ex:PersonShape a sh:NodeShape ; sh:targetClass ex:Person ; {body} .")
}

/// A Person must carry exactly one string `ex:fullName`.
fn full_name_shape() -> String {
    shape("sh:property [ sh:path ex:fullName ; sh:minCount 1 ; sh:datatype xsd:string ]")
}

fn propose(store: &Store, diff: &str, program: &[MigrationOp]) -> i64 {
    let id = store
        .insert_proposal(&NewProposal {
            kind: &ProposalKind::Shape,
            target: "people",
            diff,
            rationale: None,
            proposer: "agent/test",
            trigger_ref: None,
            created_at: T0,
        })
        .unwrap();
    store.set_proposal_migration(id, program).unwrap();
    id
}

fn objects(store: &Store, subject: &str, predicate: &str) -> Vec<Value> {
    let q = format!("SELECT ?o WHERE {{ <{subject}> <{predicate}> ?o }}");
    query(store, &q)
        .unwrap()
        .rows()
        .iter()
        .map(|r| r["o"].clone())
        .collect()
}

fn objects_at(store: &Store, subject: &str, predicate: &str, valid_at: &str) -> Vec<Value> {
    let q = format!("SELECT ?o WHERE {{ <{subject}> <{predicate}> ?o }}");
    let ctx = TemporalContext {
        valid_at: Some(valid_at.into()),
        ..TemporalContext::default()
    };
    query_temporal(store, &q, &ctx)
        .unwrap()
        .rows()
        .iter()
        .map(|r| r["o"].clone())
        .collect()
}

fn migration_txs(store: &Store, id: i64) -> i64 {
    store
        .conn
        .query_row(
            "SELECT COUNT(*) FROM transactions WHERE source = ?1",
            [migration_source(id)],
            |r| r.get(0),
        )
        .unwrap()
}

fn s(v: &str) -> Value {
    Value::Str(v.into())
}

fn rename() -> MigrationOp {
    MigrationOp::RenamePredicate {
        from: ex("name"),
        to: ex("fullName"),
    }
}

#[test]
fn rename_predicate_supersedes_old_facts_in_one_transaction() {
    let mut store = store_with(PEOPLE);
    let id = propose(&store, &full_name_shape(), &[rename()]);
    let (proposal, tx) = store
        .accept_proposal_with_migration(id, "approver", DECIDED, None)
        .unwrap();
    assert_eq!(proposal.status, ProposalStatus::Accepted);
    assert!(tx.is_some());
    assert_eq!(
        migration_txs(&store, id),
        1,
        "the migration is ONE transaction"
    );
    assert_eq!(
        objects(&store, &ex("alice"), &ex("fullName")),
        vec![s("Alice Smith")]
    );
    assert!(objects(&store, &ex("alice"), &ex("name")).is_empty());
    // As-of before the migration still reads the old facts, and not the new.
    assert_eq!(
        objects_at(&store, &ex("alice"), &ex("name"), BEFORE),
        vec![s("Alice Smith")]
    );
    assert!(objects_at(&store, &ex("alice"), &ex("fullName"), BEFORE).is_empty());
    // The old fact is closed at the decision instant, not rewritten.
    let (valid_from, valid_to): (String, Option<String>) = store
        .conn
        .query_row(
            "SELECT f.valid_from, f.valid_to FROM facts f \
             WHERE f.e = ?1 AND f.a = ?2 AND f.op = 1",
            [
                store.lookup(&ex("alice")).unwrap().unwrap(),
                store.lookup(&ex("name")).unwrap().unwrap(),
            ],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(valid_from, T0);
    assert_eq!(valid_to.as_deref(), Some(DECIDED));
}

#[test]
fn retype_class_moves_instances() {
    let mut store = store_with(PEOPLE);
    let program = [MigrationOp::RetypeClass {
        from: ex("Person"),
        to: ex("Human"),
    }];
    let human = format!(
        "{PREFIXES}ex:HumanShape a sh:NodeShape ; sh:targetClass ex:Human ; sh:property [ sh:path ex:name ] ."
    );
    let id = propose(&store, &human, &program);
    store
        .accept_proposal(id, "approver", DECIDED, None)
        .unwrap();
    let ty = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";
    let human = store.lookup(&ex("Human")).unwrap().unwrap();
    assert_eq!(objects(&store, &ex("bob"), ty), vec![Value::Ref(human)]);
    let person = store.lookup(&ex("Person")).unwrap().unwrap();
    assert_eq!(
        objects_at(&store, &ex("bob"), ty, BEFORE),
        vec![Value::Ref(person)]
    );
}

#[test]
fn cast_datatype_writes_canonical_values() {
    let mut store = store_with(PEOPLE);
    let program = [MigrationOp::CastDatatype {
        predicate: ex("age"),
        datatype: format!("{}integer", crate::namespace::XSD),
    }];
    let id = propose(
        &store,
        &shape("sh:property [ sh:path ex:age ; sh:datatype xsd:integer ]"),
        &program,
    );
    store
        .accept_proposal(id, "approver", DECIDED, None)
        .unwrap();
    assert_eq!(
        objects(&store, &ex("alice"), &ex("age")),
        vec![Value::Int(42)]
    );
    assert_eq!(
        objects_at(&store, &ex("alice"), &ex("age"), BEFORE),
        vec![s("42")]
    );
}

#[test]
fn split_property_computes_each_target() {
    let mut store = store_with(PEOPLE);
    let program = [MigrationOp::SplitProperty {
        from: ex("name"),
        into: vec![
            SplitTarget {
                predicate: ex("given"),
                expression: "STRBEFORE(?o, \" \")".into(),
            },
            SplitTarget {
                predicate: ex("family"),
                expression: "STRAFTER(?o, \" \")".into(),
            },
        ],
    }];
    let id = propose(&store, &shape("sh:property [ sh:path ex:given ]"), &program);
    store
        .accept_proposal(id, "approver", DECIDED, None)
        .unwrap();
    assert_eq!(
        objects(&store, &ex("alice"), &ex("given")),
        vec![s("Alice")]
    );
    assert_eq!(
        objects(&store, &ex("alice"), &ex("family")),
        vec![s("Smith")]
    );
    assert!(objects(&store, &ex("alice"), &ex("name")).is_empty());
}

#[test]
fn merge_property_combines_or_moves_values() {
    let turtle = "@prefix ex: <http://example.org/> .\n\
                  ex:alice ex:given \"Alice\" ; ex:family \"Smith\" ; ex:nick \"Al\" ; ex:alias \"A\" .";
    let mut store = store_with(turtle);
    let program = [
        MigrationOp::MergeProperty {
            from: vec![ex("given"), ex("family")],
            to: ex("fullName"),
            expression: Some("CONCAT(?v0, \" \", ?v1)".into()),
        },
        MigrationOp::MergeProperty {
            from: vec![ex("nick"), ex("alias")],
            to: ex("aka"),
            expression: None,
        },
    ];
    let id = propose(
        &store,
        &shape("sh:property [ sh:path ex:fullName ]"),
        &program,
    );
    store
        .accept_proposal(id, "approver", DECIDED, None)
        .unwrap();
    assert_eq!(
        objects(&store, &ex("alice"), &ex("fullName")),
        vec![s("Alice Smith")]
    );
    let mut aka = objects(&store, &ex("alice"), &ex("aka"));
    aka.sort_by_key(|v| format!("{v:?}"));
    assert_eq!(aka, vec![s("A"), s("Al")]);
    for gone in ["given", "family", "nick", "alias"] {
        assert!(
            objects(&store, &ex("alice"), &ex(gone)).is_empty(),
            "{gone}"
        );
    }
}

#[test]
fn raw_sparql_template_runs() {
    let mut store = store_with(PEOPLE);
    let program = [MigrationOp::Sparql {
        update: format!(
            "DELETE {{ ?s <{EX}age> ?a }} INSERT {{ ?s <{EX}years> ?a }} WHERE {{ ?s <{EX}age> ?a }}"
        ),
    }];
    let id = propose(&store, &shape("sh:property [ sh:path ex:years ]"), &program);
    store
        .accept_proposal(id, "approver", DECIDED, None)
        .unwrap();
    assert_eq!(objects(&store, &ex("bob"), &ex("years")), vec![s("7")]);
}

fn assert_untouched(store: &Store, id: i64) {
    let proposal = store.get_proposal(id).unwrap().unwrap();
    assert_eq!(proposal.status, ProposalStatus::Pending);
    assert!(
        store.list_shapes().unwrap().is_empty(),
        "shapes must not load"
    );
    assert_eq!(migration_txs(store, id), 0);
    assert_eq!(
        objects(store, &ex("alice"), &ex("name")),
        vec![s("Alice Smith")]
    );
    assert!(objects(store, &ex("alice"), &ex("fullName")).is_empty());
    assert_eq!(objects(store, &ex("alice"), &ex("age")), vec![s("42")]);
}

#[test]
fn a_failing_op_leaves_nothing_written() {
    let mut store = store_with(PEOPLE);
    // The rename alone would succeed; the cast of "Alice Smith" to an integer
    // must fail the whole program.
    let program = [
        rename(),
        MigrationOp::CastDatatype {
            predicate: ex("name"),
            datatype: format!("{}integer", crate::namespace::XSD),
        },
    ];
    let id = propose(&store, &full_name_shape(), &program);
    let err = store
        .accept_proposal(id, "approver", DECIDED, None)
        .unwrap_err()
        .to_string();
    assert!(err.contains("ill-typed"), "{err}");
    assert_untouched(&store, id);
}

#[test]
fn a_migration_the_new_shapes_refuse_leaves_nothing_written() {
    let mut store = store_with(PEOPLE);
    // The new shape wants an integer fullName; renaming strings into it fails.
    let diff = shape("sh:property [ sh:path ex:fullName ; sh:datatype xsd:integer ]");
    let id = propose(&store, &diff, &[rename()]);
    let err = store
        .accept_proposal(id, "approver", DECIDED, None)
        .unwrap_err()
        .to_string();
    assert!(err.contains("remains pending"), "{err}");
    assert!(err.contains("SHACL"), "{err}");
    assert_untouched(&store, id);
    // The dry run predicted exactly that refusal.
    let report = dry_run(&store, id, 10).unwrap();
    assert_eq!(report["gate"]["would_accept"], false);
}

#[test]
fn dry_run_reports_refusals_and_matches_the_accept() {
    let mut store = store_with(PEOPLE);
    let id = propose(&store, &full_name_shape(), &[rename()]);
    let report = dry_run(&store, id, 10).unwrap();
    // Both people lack ex:fullName under the new shape today.
    assert_eq!(
        report["refused_before_migration"]["violations"], 2,
        "{report}"
    );
    let focus: Vec<_> = report["refused_before_migration"]["results"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["focus_node"].as_str().unwrap().to_owned())
        .collect();
    assert!(focus.iter().any(|f| f.contains("alice")), "{focus:?}");
    assert_eq!(report["migration"]["retracted"], 2);
    assert_eq!(report["migration"]["asserted"], 2);
    assert_eq!(report["migration"]["ops"][0]["op"], "rename_predicate");
    assert_eq!(
        report["refused_after_migration"]["violations"], 0,
        "{report}"
    );
    assert_eq!(report["gate"]["would_accept"], true);
    // The dry run wrote nothing.
    assert_untouched(&store, id);

    store
        .accept_proposal(id, "approver", DECIDED, None)
        .unwrap();
    let (asserted, retracted): (i64, i64) = store
        .conn
        .query_row(
            "SELECT SUM(f.op = 1 AND f.valid_to IS NULL), SUM(f.op = 0) FROM facts f \
             JOIN transactions t ON f.tx = t.id WHERE t.source = ?1",
            [migration_source(id)],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(asserted, 2);
    assert_eq!(retracted, 2);
}

#[test]
fn dry_run_refuses_a_decided_proposal() {
    let mut store = store_with(PEOPLE);
    let id = propose(&store, &full_name_shape(), &[rename()]);
    store
        .accept_proposal(id, "approver", DECIDED, None)
        .unwrap();
    assert!(dry_run(&store, id, 10).is_err());
}

#[test]
fn programs_are_checked_before_anything_is_stored() {
    let bad = [
        serde_json::json!([]),
        serde_json::json!([{ "op": "rename_predicate", "from": "not an iri", "to": "http://x/y" }]),
        serde_json::json!([{ "op": "teleport" }]),
        serde_json::json!([{ "op": "sparql", "update": "INSERT DATA { GRAPH <http://g/> { <http://a/> <http://b/> 1 } }" }]),
        serde_json::json!([{ "op": "sparql", "update": "CLEAR ALL" }]),
        serde_json::json!([{ "op": "sparql", "update": "INSERT { _:b <http://b/> ?o } WHERE { ?s <http://a/> ?o }" }]),
    ];
    let store = Store::open_in_memory().unwrap();
    for program in bad {
        let input = serde_json::json!({
            "kind": "shape", "target": "t", "diff": full_name_shape(),
            "proposer": "agent", "migration": program,
        });
        assert!(
            crate::mcp::proposal::tool_propose_schema_change(&store, &input).is_err(),
            "{program}"
        );
    }
    assert!(store.list_proposals(None).unwrap().is_empty());
}

#[test]
fn mcp_round_trip_propose_dry_run_accept() {
    use crate::mcp::proposal::{
        tool_accept_proposal, tool_list_proposals, tool_propose_schema_change,
    };
    let mut store = store_with(PEOPLE);
    let out = tool_propose_schema_change(
        &store,
        &serde_json::json!({
            "kind": "shape", "target": "people", "diff": full_name_shape(),
            "proposer": "agent",
            "migration": [{ "op": "rename_predicate", "from": ex("name"), "to": ex("fullName") }],
        }),
    )
    .unwrap();
    let id = out["proposal_id"].as_i64().unwrap();
    assert_eq!(out["migration_ops"], 1);
    let listed = tool_list_proposals(&store, &serde_json::json!({})).unwrap();
    assert_eq!(
        listed["proposals"][0]["migration"][0]["op"],
        "rename_predicate"
    );
    let report = tool_accept_proposal(
        &mut store,
        &serde_json::json!({ "id": id, "decided_by": "approver", "dry_run": true }),
    )
    .unwrap();
    assert_eq!(report["status"], "pending");
    assert_eq!(report["migration"]["asserted"], 2);
    let accepted = tool_accept_proposal(
        &mut store,
        &serde_json::json!({ "id": id, "decided_by": "approver", "timestamp": DECIDED }),
    )
    .unwrap();
    assert_eq!(accepted["migration_source"], migration_source(id));
    assert!(accepted["migration_tx"].is_i64(), "{accepted}");
}

/// Review of #896: an expression that closes the BIND and smuggles in a
/// store-wide `DELETE WHERE` must be refused at submit and never run.
#[test]
fn an_expression_cannot_inject_update_operations() {
    let exploit = "STR(?o)) AS ?split0) } ; DELETE WHERE { ?a ?b ?c } ; \
                   INSERT DATA { <x> <y> \"z\" } ; DELETE { ?s <q> ?o } WHERE { ?s <name> ?o BIND((1";
    let split = serde_json::json!([{
        "op": "split_property", "from": ex("name"),
        "into": [{ "predicate": ex("given"), "expression": exploit }],
    }]);
    let merge = serde_json::json!([{
        "op": "merge_property", "from": [ex("name")], "to": ex("fullName"),
        "expression": exploit,
    }]);
    let exists = serde_json::json!([{
        "op": "split_property", "from": ex("name"),
        "into": [{ "predicate": ex("given"),
                   "expression": "IF(EXISTS { GRAPH ?g { ?a ?b ?c } }, ?o, ?o)" }],
    }]);
    for program in [split, merge, exists] {
        let err = parse_program(&program).unwrap_err().to_string();
        assert!(err.contains("expression"), "{err}");
    }
    // Run directly (as a stored program would be), it still refuses and the
    // store is untouched.
    let mut store = store_with(PEOPLE);
    let op = MigrationOp::SplitProperty {
        from: ex("name"),
        into: vec![SplitTarget {
            predicate: ex("given"),
            expression: exploit.into(),
        }],
    };
    let id = propose(&store, &full_name_shape(), &[op]);
    assert!(
        store
            .accept_proposal(id, "approver", DECIDED, None)
            .is_err()
    );
    assert_untouched(&store, id);
}

#[test]
fn a_typed_expression_is_reserialized_not_pasted() {
    let op = MigrationOp::SplitProperty {
        from: ex("name"),
        into: vec![SplitTarget {
            predicate: ex("given"),
            expression: "STRBEFORE(?o, \" \") # trailing comment".into(),
        }],
    };
    let update = op.checked_render().unwrap();
    assert!(!update.contains('#'), "{update}");
}

/// Review of #896: templates read ROOT only, so GRAPH and SERVICE are refused
/// in the WHERE pattern, including inside EXISTS.
#[test]
fn templates_may_not_read_named_graphs() {
    for update in [
        "INSERT { ?s <http://x/leaked> ?o } WHERE { GRAPH <http://x/g> { ?s <http://x/secret> ?o } }",
        "INSERT { ?s <http://x/leaked> ?o } WHERE { GRAPH ?g { ?s <http://x/secret> ?o } }",
        "INSERT { ?s <http://x/l> ?o } WHERE { ?s <http://x/p> ?o FILTER EXISTS { GRAPH ?g { ?s ?p ?o } } }",
        "INSERT { ?s <http://x/l> ?o } WHERE { SERVICE <http://x/sparql> { ?s <http://x/p> ?o } }",
    ] {
        let program = serde_json::json!([{ "op": "sparql", "update": update }]);
        let err = parse_program(&program).unwrap_err().to_string();
        assert!(err.contains("GRAPH") || err.contains("SERVICE"), "{err}");
    }
}

#[test]
fn a_migration_over_the_ceiling_is_refused_in_accept_and_dry_run() {
    let mut store = store_with(PEOPLE);
    let id = propose(&store, &full_name_shape(), &[rename()]);
    guard::with_max_facts(1, || {
        let err = dry_run(&store, id, 10).unwrap_err().to_string();
        assert!(err.contains("update_full_copy_max_facts"), "{err}");
        let err = store
            .accept_proposal(id, "approver", DECIDED, None)
            .unwrap_err()
            .to_string();
        assert!(err.contains("update_full_copy_max_facts"), "{err}");
    });
    assert_untouched(&store, id);
    // Two matches and four planned facts: a ceiling of 3 still refuses the
    // program as a whole.
    guard::with_max_facts(3, || {
        let err = dry_run(&store, id, 10).unwrap_err().to_string();
        assert!(err.contains("would change"), "{err}");
    });
    guard::with_max_facts(4, || {
        store
            .accept_proposal(id, "approver", DECIDED, None)
            .unwrap();
    });
}

/// Deduplicating the final change set must not hide oversized intermediate work.
#[test]
fn repeated_template_facts_are_bounded_before_netting() {
    let store = store_with(PEOPLE);
    let triple = "<http://example.org/new> <http://example.org/p> 1 . ";
    let cases = [
        vec![MigrationOp::Sparql {
            update: format!("INSERT DATA {{ {} }}", triple.repeat(4)),
        }],
        vec![MigrationOp::Sparql {
            update: format!("INSERT {{ {} }} WHERE {{}}", triple.repeat(4)),
        }],
        vec![
            MigrationOp::Sparql {
                update: format!("INSERT DATA {{ {triple} }}"),
            };
            4
        ],
    ];
    for program in cases {
        guard::with_max_facts(3, || {
            let err = plan(&store, &program).unwrap_err().to_string();
            assert!(err.contains("update_full_copy_max_facts"), "{err}");
        });
        guard::with_max_facts(4, || {
            assert_eq!(plan(&store, &program).unwrap().assert_count(), 1);
        });
    }
}

/// A late filter must not hide oversized WHERE intermediates behind LIMIT.
#[test]
fn filtered_where_intermediates_share_the_migration_ceiling() {
    let facts = (0..10)
        .map(|i| format!("<http://example.org/item/{i}> <http://example.org/p> {i} .\n"))
        .collect::<String>();
    let mut store = store_with(&facts);
    let program = [MigrationOp::Sparql {
        update: "INSERT { ?s <http://example.org/q> ?o } WHERE { ?s <http://example.org/p> ?o FILTER(?o + 1 > 100) }".into(),
    }];
    guard::with_max_facts(3, || {
        let err = plan(&store, &program).unwrap_err().to_string();
        assert!(err.contains("update_full_copy_max_facts"), "{err}");
    });
    guard::with_max_facts(10, || {
        assert_eq!(plan(&store, &program).unwrap().assert_count(), 0);
    });
    store.search_config_mut().max_join_rows = 2;
    guard::with_max_facts(10, || {
        let err = plan(&store, &program).unwrap_err().to_string();
        assert!(err.contains("max_join_rows"), "{err}");
    });
}

// Lex non-author probes: isolated stores, no remote calls or runtime adoption.
#[test]
fn lex_review_cast_rejects_invalid_xsd_value_spaces() {
    let mut invalid_accepted = Vec::new();
    for (lexical, datatype) in [
        ("-1", "unsignedInt"),
        ("256", "unsignedByte"),
        ("0", "positiveInteger"),
        ("2026-02-31", "date"),
    ] {
        let turtle = format!("@prefix ex: <{EX}> . ex:alice a ex:Person; ex:age \"{lexical}\" .");
        let mut store = store_with(&turtle);
        let id = propose(
            &store,
            &shape("sh:closed false"),
            &[MigrationOp::CastDatatype {
                predicate: ex("age"),
                datatype: format!("{}{datatype}", crate::namespace::XSD),
            }],
        );
        let preview = dry_run(&store, id, 10);
        let accepted = store.accept_proposal(id, "lex-review-fixture", DECIDED, None);
        eprintln!(
            "CAST {lexical:?} -> xsd:{datatype}: preview={preview:?}; acceptance={accepted:?}; stored={:?}",
            objects(&store, &ex("alice"), &ex("age"))
        );
        if accepted.is_ok() {
            invalid_accepted.push((lexical, datatype));
        }
    }
    assert!(
        invalid_accepted.is_empty(),
        "invalid XSD values accepted and persisted: {invalid_accepted:?}"
    );
}

#[test]
fn lex_review_valid_xsd_cast_control() {
    let mut store =
        store_with("@prefix ex: <http://example.org/> . ex:alice a ex:Person; ex:age \"255\" .");
    let id = propose(
        &store,
        &shape("sh:closed false"),
        &[MigrationOp::CastDatatype {
            predicate: ex("age"),
            datatype: format!("{}unsignedByte", crate::namespace::XSD),
        }],
    );
    assert_eq!(
        dry_run(&store, id, 10).unwrap()["gate"]["would_accept"],
        true
    );
    store
        .accept_proposal(id, "lex-review-fixture", DECIDED, None)
        .unwrap();
    assert!(
        matches!(&objects(&store, &ex("alice"), &ex("age"))[0], Value::Typed { lexical, datatype } if lexical == "255" && datatype.ends_with("unsignedByte"))
    );
}

#[test]
fn lex_review_dry_run_matches_validation_off_delete() {
    let mut store = store_with(PEOPLE);
    store.shacl_config_mut().validate_on_write = false;
    let id = propose(
        &store,
        &shape("sh:property [ sh:path ex:name; sh:minCount 1 ]"),
        &[MigrationOp::Sparql {
            update: format!(
                "DELETE DATA {{ <{}> <{}> \"Alice Smith\" }}",
                ex("alice"),
                ex("name")
            ),
        }],
    );
    let report = dry_run(&store, id, 10).unwrap();
    let acceptance = store.accept_proposal(id, "lex-review-fixture", DECIDED, None);
    eprintln!("validation_off preview={report}; acceptance={acceptance:?}");
    assert_eq!(
        report["gate"]["would_accept"].as_bool().unwrap(),
        acceptance.is_ok(),
        "dry-run must agree with actual acceptance under the same config"
    );
}

#[test]
fn lex_review_refused_commit_leaves_connection_usable() {
    let mut store = store_with(PEOPLE);
    let id = propose(&store, &full_name_shape(), &[rename()]);
    // The existing ownership fence produces precisely this SQLite veto. This
    // in-memory hook injects the same code at the outermost RELEASE, offline.
    store.conn.commit_hook(Some(|| true)).unwrap();
    let result = store.accept_proposal(id, "lex-review-fixture", DECIDED, None);
    eprintln!(
        "commit veto result={result:?}; autocommit={}",
        store.conn.is_autocommit()
    );
    assert!(result.is_err());
    assert!(
        store.conn.is_autocommit(),
        "refused outer RELEASE must settle the dead transaction"
    );
    store.conn.commit_hook(None::<fn() -> bool>).unwrap();
    assert_eq!(
        store.get_proposal(id).unwrap().unwrap().status,
        ProposalStatus::Pending
    );
    let next = propose(&store, &full_name_shape(), &[rename()]);
    assert_eq!(
        store.get_proposal(next).unwrap().unwrap().status,
        ProposalStatus::Pending
    );
    assert!(
        store.conn.is_autocommit(),
        "subsequent write commits normally"
    );
}

#[test]
fn lex_review_decided_program_is_immutable() {
    let mut store = store_with(PEOPLE);
    let original = vec![rename()];
    let id = propose(&store, &full_name_shape(), &original);
    store
        .accept_proposal(id, "lex-review-fixture", DECIDED, None)
        .unwrap();
    let replacement = [MigrationOp::Sparql {
        update: "DELETE DATA { <http://example.org/x> <http://example.org/p> 1 }".into(),
    }];
    let changed = store.set_proposal_migration(id, &replacement);
    eprintln!(
        "post-accept replacement={changed:?}; now={:?}",
        store.get_proposal_migration(id).unwrap()
    );
    assert!(
        changed.is_err(),
        "accepted program must remain the one whose migration transaction was committed"
    );
    assert_eq!(store.get_proposal_migration(id).unwrap(), Some(original));
}

#[test]
fn lex_review_existing_release_helper_settles_same_veto() {
    let store = store_with(PEOPLE);
    let id = propose(&store, &full_name_shape(), &[rename()]);
    store
        .conn
        .execute_batch("SAVEPOINT lex_review_control")
        .unwrap();
    store
        .conn
        .execute(
            "UPDATE proposals SET decision_note='control' WHERE id=?1",
            [id],
        )
        .unwrap();
    store.conn.commit_hook(Some(|| true)).unwrap();
    assert!(crate::store::release::release(&store.conn, "lex_review_control").is_err());
    assert!(
        store.conn.is_autocommit(),
        "existing release helper must settle the same veto"
    );
    assert!(
        store
            .get_proposal(id)
            .unwrap()
            .unwrap()
            .decision_note
            .is_none()
    );
}
