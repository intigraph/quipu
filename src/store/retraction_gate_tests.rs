//! aegis-gxfm: every native retract path runs the deletion gate.
//!
//! Each entry point gets the same three checks: retracting a value a reject
//! shape's `sh:minCount 1` requires is refused with nothing written; with
//! `shacl.validate_on_write` off the old behaviour returns; an emit shape
//! never blocks (it queues a `shacl.violation` event instead).

use serde_json::json;

use super::*;
use crate::store::forks::ForkPromotion;
use crate::types::{Op, Value};

const TS: &str = "2026-10-28T00:00:00Z";
const CAROL: &str = "urn:t:carol";
const NAME: &str = "urn:t:name";
const NICK: &str = "urn:t:nick";

const REJECT: &str = r#"
@prefix sh: <http://www.w3.org/ns/shacl#> .
@prefix ex: <urn:t:> .
ex:PersonShape a sh:NodeShape ;
    sh:targetClass ex:Person ;
    sh:property [ sh:path ex:name ; sh:minCount 1 ; sh:message "a Person has a name" ] .
"#;

const EMIT: &str = r#"
@prefix sh: <http://www.w3.org/ns/shacl#> .
@prefix ex: <urn:t:> .
@prefix q: <http://quipu.dev/ns#> .
ex:PersonShape a sh:NodeShape ;
    sh:targetClass ex:Person ; q:onViolation "emit" ;
    sh:property [ sh:path ex:name ; sh:minCount 1 ; sh:message "a Person has a name" ] .
"#;

fn datum(store: &mut Store, s: &str, p: &str, v: Value, op: Op) -> Datum {
    Datum {
        entity: store.intern(s).unwrap(),
        attribute: store.intern(p).unwrap(),
        value: v,
        valid_from: TS.into(),
        valid_to: None,
        op,
    }
}

fn person_type(store: &mut Store) -> Datum {
    let person = Value::Ref(store.intern("urn:t:Person").unwrap());
    datum(store, CAROL, crate::namespace::RDF_TYPE, person, Op::Assert)
}

fn name(store: &mut Store, op: Op) -> Datum {
    datum(store, CAROL, NAME, Value::Str("Carol".into()), op)
}

/// carol: a Person with a name and a nick, all under `source`.
fn store_with(shapes: &str, source: &str) -> Store {
    let mut store = Store::open_in_memory().unwrap();
    let nick = datum(&mut store, CAROL, NICK, Value::Str("C".into()), Op::Assert);
    let seed = [person_type(&mut store), name(&mut store, Op::Assert), nick];
    store.transact(&seed, TS, None, Some(source)).unwrap();
    store.load_shapes("people", shapes, TS).unwrap();
    store.shacl_config_mut().validate_on_write = true;
    store
}

fn names(store: &Store) -> usize {
    let carol = store.lookup(CAROL).unwrap().unwrap();
    let name = store.lookup(NAME).unwrap().unwrap();
    store
        .entity_facts(carol)
        .unwrap()
        .iter()
        .filter(|f| f.attribute == name)
        .count()
}

fn violation_events(store: &Store) -> usize {
    store
        .events_after(0, 1000, None, None)
        .unwrap()
        .into_iter()
        .filter(|e| e.event_type == "shacl.violation")
        .count()
}

fn assert_refused(store: &Store, err: &crate::Error, tx_before: i64) {
    let msg = err.to_string();
    assert!(msg.contains("introduced by a deletion"), "{msg}");
    assert!(msg.contains("a Person has a name"), "{msg}");
    assert_eq!(store.latest_tx_id().unwrap(), tx_before, "nothing written");
    assert_eq!(names(store), 1);
}

fn retract_name(store: &mut Store) -> crate::Result<serde_json::Value> {
    crate::tool_retract(store, &json!({"entity": CAROL, "predicate": NAME}))
}

// ---------------------------------------------------------------- quipu_retract

#[test]
fn retract_of_a_required_value_is_refused() {
    let mut store = store_with(REJECT, "seed");
    let tx = store.latest_tx_id().unwrap();
    let err = retract_name(&mut store).unwrap_err();
    assert_refused(&store, &err, tx);
    assert!(
        store
            .refusals_by_gate()
            .unwrap()
            .iter()
            .any(|(g, n)| g == "shacl" && *n == 1)
    );
    // An optional value still retracts, and so does the whole entity.
    let out = crate::tool_retract(&mut store, &json!({"entity": CAROL, "predicate": NICK}));
    assert_eq!(out.unwrap()["retracted"], 1);
    let out = crate::tool_retract(&mut store, &json!({"entity": CAROL})).unwrap();
    assert_eq!(out["retracted"], 2, "whole-node retraction is not judged");
}

#[test]
fn retract_with_validate_on_write_off_is_the_old_behaviour() {
    let mut store = store_with(REJECT, "seed");
    store.shacl_config_mut().validate_on_write = false;
    assert_eq!(retract_name(&mut store).unwrap()["retracted"], 1);
    assert_eq!(names(&store), 0);
}

#[test]
fn retract_under_an_emit_shape_proceeds_and_queues_the_violation() {
    let mut store = store_with(EMIT, "seed");
    assert_eq!(retract_name(&mut store).unwrap()["retracted"], 1);
    assert_eq!(names(&store), 0);
    assert_eq!(violation_events(&store), 1);
}

#[test]
fn retract_through_the_python_binding_engine_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("q.db");
    let path = db.to_str().unwrap();
    {
        let mut store = Store::open(path).unwrap();
        let seed = [person_type(&mut store), name(&mut store, Op::Assert)];
        store.transact(&seed, TS, None, Some("seed")).unwrap();
        store.load_shapes("people", REJECT, TS).unwrap();
    }
    let mut engine = crate::bindings::Engine::open(path).unwrap();
    let err = engine
        .invoke(
            "quipu_retract",
            &json!({"entity": CAROL, "predicate": NAME}),
        )
        .unwrap_err();
    assert!(
        format!("{err:?}").contains("introduced by a deletion"),
        "{err:?}"
    );
    drop(engine);
    assert_eq!(names(&Store::open(path).unwrap()), 1);
}

// -------------------------------------------------------- quipu_retract_episode

/// carol's type comes from `seed`; episode `e1` contributed her name.
fn episode_store(shapes: &str) -> Store {
    let mut store = Store::open_in_memory().unwrap();
    let t = person_type(&mut store);
    store.transact(&[t], TS, None, Some("seed")).unwrap();
    let n = name(&mut store, Op::Assert);
    store.transact(&[n], TS, None, Some("episode:e1")).unwrap();
    store.load_shapes("people", shapes, TS).unwrap();
    store.shacl_config_mut().validate_on_write = true;
    store
}

#[test]
fn retract_episode_leaving_a_typed_node_without_a_required_value_is_refused() {
    let mut store = episode_store(REJECT);
    let tx = store.latest_tx_id().unwrap();
    let err = crate::tool_retract_episode(&mut store, &json!({"episode": "e1"})).unwrap_err();
    assert_refused(&store, &err, tx);
    // `on_orphan` is about identity, not shapes: it is no bypass.
    let err =
        crate::tool_retract_episode(&mut store, &json!({"episode": "e1", "on_orphan": "allow"}))
            .unwrap_err();
    assert_refused(&store, &err, tx);
}

#[test]
fn retract_episode_removing_the_node_whole_is_allowed() {
    let mut store = store_with(REJECT, "episode:e2");
    let out = crate::tool_retract_episode(&mut store, &json!({"episode": "e2"})).unwrap();
    assert_eq!(out["retracted"], 3);
}

#[test]
fn retract_episode_keeps_a_value_another_producer_also_claims() {
    let mut store = episode_store(REJECT);
    // A second producer claims the same name: the episode's retraction leaves
    // it visible, so it is not judged as removed.
    let n = name(&mut store, Op::Assert);
    store.transact(&[n], TS, None, Some("other")).unwrap();
    crate::tool_retract_episode(&mut store, &json!({"episode": "e1"})).unwrap();
    assert_eq!(names(&store), 1);
}

#[test]
fn retract_episode_with_validate_on_write_off_and_under_emit() {
    let mut store = episode_store(REJECT);
    store.shacl_config_mut().validate_on_write = false;
    let out = crate::tool_retract_episode(&mut store, &json!({"episode": "e1"})).unwrap();
    assert_eq!(out["retracted"], 1);

    let mut store = episode_store(EMIT);
    let out = crate::tool_retract_episode(&mut store, &json!({"episode": "e1"})).unwrap();
    assert_eq!(out["retracted"], 1);
    assert_eq!(violation_events(&store), 1);
}

// --------------------------------------------------------- quipu_retract_source

fn retract_source(
    store: &mut Store,
    source: &str,
    expect: i64,
) -> crate::Result<serde_json::Value> {
    crate::tool_retract_source(
        store,
        &json!({"source": source, "repair": "aegis-gxfm", "apply": true, "expect": expect}),
    )
}

#[test]
fn retract_source_leaving_a_required_value_missing_is_refused() {
    let mut store = episode_store(REJECT);
    let tx = store.latest_tx_id().unwrap();
    let err = retract_source(&mut store, "episode:e1", 1).unwrap_err();
    assert_refused(&store, &err, tx);
}

#[test]
fn retract_source_with_validate_on_write_off_and_under_emit() {
    let mut store = episode_store(REJECT);
    store.shacl_config_mut().validate_on_write = false;
    assert_eq!(
        retract_source(&mut store, "episode:e1", 1).unwrap()["retracted"],
        1
    );

    let mut store = episode_store(EMIT);
    assert_eq!(
        retract_source(&mut store, "episode:e1", 1).unwrap()["retracted"],
        1
    );
    assert_eq!(violation_events(&store), 1);
}

// --------------------------------------------------------------- fork promote

#[test]
fn fork_promote_that_only_removes_a_required_value_is_refused() {
    let mut store = store_with(REJECT, "seed");
    let latest = store.latest_tx_id().unwrap();
    let fork = store.fork_create("strip", latest, TS, None).unwrap();
    let gone = name(&mut store, Op::Retract);
    store
        .transact_to_graph(&[gone], TS, None, None, fork.g)
        .unwrap();
    let tx = store.latest_tx_id().unwrap();
    match store.fork_promote("strip", TS, None).unwrap() {
        ForkPromotion::Refused(feedback) => {
            assert!(!feedback.conforms);
            assert!(
                feedback.results[0]
                    .component
                    .ends_with("MinCountConstraintComponent")
            );
        }
        other => panic!("expected a deletion refusal, got {other:?}"),
    }
    assert_eq!(store.latest_tx_id().unwrap(), tx);
    assert_eq!(names(&store), 1);
    assert_eq!(store.fork_lookup("strip").unwrap().unwrap().status, "open");

    // The old behaviour with the flag off.
    store.shacl_config_mut().validate_on_write = false;
    assert!(matches!(
        store.fork_promote("strip", TS, None).unwrap(),
        ForkPromotion::Promoted { retracted: 1, .. }
    ));
    assert_eq!(names(&store), 0);
}

// ------------------------------------------------------------- blank nodes

/// A blank-node subject is written `_:b`, never `<_:b>`: the gate must not
/// turn every retraction touching one into a parse error.
#[test]
fn retraction_touching_a_blank_node_is_judged_not_a_parse_error() {
    let mut store = Store::open_in_memory().unwrap();
    let carol = store.intern(CAROL).unwrap();
    let nick = datum(&mut store, "_:b1", NICK, Value::Str("x".into()), Op::Assert);
    let edge = datum(&mut store, "_:b1", NAME, Value::Ref(carol), Op::Assert);
    store
        .transact(&[nick, edge], TS, None, Some("episode:blank"))
        .unwrap();
    store.load_shapes("people", REJECT, TS).unwrap();
    store.shacl_config_mut().validate_on_write = true;
    let b1 = store.lookup("_:b1").unwrap().unwrap();
    let nick = store.lookup(NICK).unwrap().unwrap();
    store.retract_entity(b1, Some(nick), TS, None).unwrap();
    let out = crate::tool_retract_episode(&mut store, &json!({"episode": "blank"})).unwrap();
    assert_eq!(out["retracted"], 1);
}

/// A judged node that points to a TYPED blank node: the referenced node's
/// type is context, written `_:b a <T>`, never `<_:b> a <T>` (an N-Triples
/// parse error that refused the write instead of judging it).
#[test]
fn judged_node_pointing_to_a_typed_blank_node_is_judged_not_a_parse_error() {
    let mut store = store_with(REJECT, "seed");
    let pet = Value::Ref(store.intern("_:pet1").unwrap());
    let edge = datum(&mut store, CAROL, "urn:t:pet", pet, Op::Assert);
    let dog = Value::Ref(store.intern("urn:t:Dog").unwrap());
    let typed = datum(
        &mut store,
        "_:pet1",
        crate::namespace::RDF_TYPE,
        dog,
        Op::Assert,
    );
    store
        .transact(&[edge, typed], TS, None, Some("seed"))
        .unwrap();

    // Retracting the required name is judged and refused on its merits.
    let tx = store.latest_tx_id().unwrap();
    let err = retract_name(&mut store).unwrap_err();
    assert_refused(&store, &err, tx);

    // Changing it (retract + replacement assert) is judged and allowed.
    let change = [
        name(&mut store, Op::Retract),
        datum(
            &mut store,
            CAROL,
            NAME,
            Value::Str("Caz".into()),
            Op::Assert,
        ),
    ];
    store
        .enforce_retraction_shapes(&change, crate::schema::ROOT_GRAPH, None, None, None)
        .unwrap();
}

// ---------------------------------------------------- snapshot replacement

fn episode_v(owner: Option<&str>) -> serde_json::Value {
    let mut node = json!({"name": "path-a", "type": "ExecutionPath"});
    if let Some(owner) = owner {
        node["properties"] = json!({"owner": owner});
    }
    json!({"name": "paths", "nodes": [node], "replace_snapshot": true})
}

#[test]
fn episode_snapshot_that_drops_a_required_value_is_refused() {
    let ns = crate::namespace::DEFAULT_BASE_NS;
    let shapes = format!(
        "@prefix sh: <http://www.w3.org/ns/shacl#> .\n\
         <urn:t:PathShape> a sh:NodeShape ; sh:targetClass <{ns}ExecutionPath> ;\n    \
         sh:property [ sh:path <{ns}owner> ; sh:minCount 1 ] .\n"
    );
    let mut store = Store::open_in_memory().unwrap();
    store.load_shapes("paths", &shapes, TS).unwrap();
    store.shacl_config_mut().validate_on_write = true;
    crate::tool_episode(&mut store, &episode_v(Some("x"))).unwrap();
    let owner = store.lookup(&format!("{ns}owner")).unwrap().unwrap();
    let path = store.lookup(&format!("{ns}path-a")).unwrap().unwrap();
    let owners = |s: &Store| {
        s.entity_facts(path)
            .unwrap()
            .iter()
            .filter(|f| f.attribute == owner)
            .count()
    };
    assert_eq!(owners(&store), 1);
    let tx = store.latest_tx_id().unwrap();
    let err = crate::tool_episode(&mut store, &episode_v(None)).unwrap_err();
    assert!(
        err.to_string().contains("introduced by a deletion"),
        "{err}"
    );
    assert_eq!(store.latest_tx_id().unwrap(), tx);
    assert_eq!(owners(&store), 1);
    // A replacement value satisfies the floor.
    crate::tool_episode(&mut store, &episode_v(Some("y"))).unwrap();
    assert_eq!(owners(&store), 1);
}

fn knot_snapshot(store: &mut Store, body: &str) -> crate::Result<serde_json::Value> {
    crate::tool_knot(
        store,
        &json!({
            "snapshot": "people", "replace_snapshot": true,
            "turtle": format!("@prefix ex: <urn:t:> . {body}"),
        }),
    )
}

#[test]
fn knot_snapshot_that_drops_a_required_value_is_refused() {
    let mut store = Store::open_in_memory().unwrap();
    store.load_shapes("people", REJECT, TS).unwrap();
    store.shacl_config_mut().validate_on_write = true;
    knot_snapshot(&mut store, "ex:carol a ex:Person ; ex:name \"Carol\" .").unwrap();
    let tx = store.latest_tx_id().unwrap();
    let err = knot_snapshot(&mut store, "ex:carol a ex:Person .").unwrap_err();
    assert!(
        err.to_string().contains("introduced by a deletion"),
        "{err}"
    );
    assert_eq!(store.latest_tx_id().unwrap(), tx);
    assert_eq!(names(&store), 1);
}

/// The `TerminusDB` history replay: each snapshot swaps a Person's array cells
/// for new nodes while the Person's required `age` changes. The Person is
/// judged (it loses a value on a required path) and references the old
/// cells; their retracted types must not come back as context and make the
/// removed cells look typed-but-empty.
#[test]
fn snapshot_replacing_referenced_nodes_whole_is_allowed() {
    let shapes = r#"
@prefix sh: <http://www.w3.org/ns/shacl#> .
@prefix ex: <urn:t:> .
ex:PersonShape a sh:NodeShape ; sh:targetClass ex:Person ;
    sh:property [ sh:path ex:age ; sh:minCount 1 ] .
ex:CellShape a sh:NodeShape ; sh:targetClass ex:Cell ;
    sh:property [ sh:path ex:index ; sh:minCount 1 ; sh:maxCount 1 ] .
"#;
    let mut store = Store::open_in_memory().unwrap();
    store.load_shapes("cells", shapes, TS).unwrap();
    store.shacl_config_mut().validate_on_write = true;
    let version = |cell: &str, age: i64| {
        format!(
            "ex:ada a ex:Person ; ex:age {age} ; ex:scores ex:{cell} .\n\
             ex:{cell} a ex:Cell ; ex:index 0 ."
        )
    };
    knot_snapshot(&mut store, &version("old", 35)).unwrap();
    knot_snapshot(&mut store, &version("new", 36)).unwrap();
    let old = store.lookup("urn:t:old").unwrap().unwrap();
    assert!(
        store.entity_facts(old).unwrap().is_empty(),
        "old cell removed whole"
    );
    // CONTROL: keeping the old cell typed while dropping its index is refused.
    let err = knot_snapshot(
        &mut store,
        "ex:ada a ex:Person ; ex:age 37 ; ex:scores ex:new .\nex:new a ex:Cell .",
    )
    .unwrap_err();
    assert!(
        err.to_string().contains("introduced by a deletion"),
        "{err}"
    );
}
