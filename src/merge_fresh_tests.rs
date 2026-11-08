//! Fresh (parent-null) retained files are proposals. The writer binds them to
//! the real fork snapshot and the verified share manifest before any write.
use super::*;
use crate::transaction_auth::{Identity, with_identity};
use serde_json::{Value as Json, json};

const LEDGER: &str = "urn:fixture:fresh-ledger";
const SHAPES: &str =
    "@prefix sh: <http://www.w3.org/ns/shacl#> . [] sh:path <urn:p>; sh:maxCount 1 .";

fn authenticated<T>(f: impl FnOnce() -> T) -> T {
    with_identity(
        Some(Identity {
            principal: "urn:fixture:alice".into(),
            credential_id: Some("alice".into()),
            auth_class: "named_bearer".into(),
            attestation: Default::default(),
        }),
        f,
    )
}
fn assert_fact(store: &mut Store, subject: &str, value: &str, at: &str) {
    let datum = Datum {
        entity: store.intern(subject).unwrap(),
        attribute: store.intern("urn:p").unwrap(),
        value: Stored::Str(value.into()),
        valid_from: at.into(),
        valid_to: None,
        op: Op::Assert,
    };
    store.transact(&[datum], at, Some("fixture"), None).unwrap();
}
/// The retained file exactly as the profiled decisions tool emits it.
fn emitted(store: &Store, input: &Json) -> Json {
    let mut input = input.clone();
    input["profile"] = json!({});
    crate::mcp::merge_decisions::tool_merge_decisions(store, &input).unwrap()["decisions"].take()
}
fn apply(store: &mut Store, envelope: &Json, file: &Json) -> crate::Result<Json> {
    let mut input = envelope.clone();
    input["decisions"] = file.clone();
    input["ledger"] = json!(LEDGER);
    input["reviewer"] = json!("fixture");
    crate::mcp::merge_decisions::tool_merge_apply(store, &input)
}
fn refused_without_writes(store: &mut Store, envelope: &Json, file: &Json) -> String {
    let tx = store.latest_tx_id().unwrap();
    let root = crate::share_merge::root_graph(store).unwrap().0;
    let id = crate::merge_ingress::id(&crate::merge_ingress::decode(file).unwrap()).unwrap();
    let error = match apply(store, envelope, file) {
        Ok(out) => panic!("tampered fresh file was applied: {out}"),
        Err(e) => e.to_string(),
    };
    assert_eq!(store.latest_tx_id().unwrap(), tx, "no tx written");
    assert_eq!(crate::share_merge::root_graph(store).unwrap().0, root);
    assert!(
        crate::merge_pending::load(store, LEDGER, &id)
            .unwrap()
            .is_none()
    );
    error
}
fn live_starts(store: &Store, subject: &str) -> Vec<(String, String)> {
    let (Some(e), Some(a)) = (
        store.lookup(subject).unwrap(),
        store.lookup("urn:p").unwrap(),
    ) else {
        return Vec::new();
    };
    let mut out: Vec<_> = store
        .entity_facts_in_graph(e, crate::schema::ROOT_GRAPH)
        .unwrap()
        .into_iter()
        .filter(|f| f.attribute == a && f.valid_to.is_none())
        .map(|f| {
            let v = crate::rdf::value_to_term(store, &f.value).unwrap();
            (v.to_string(), f.valid_from)
        })
        .collect();
    out.sort();
    out
}

/// Gennaro's P1 case: an empty fork, then ROOT adds a fact the fork never saw.
fn fork_fixture() -> (Store, Json) {
    let mut store = Store::open_in_memory().unwrap();
    store
        .load_shapes("fresh", SHAPES, "2026-01-01T00:00:00Z")
        .unwrap();
    let tx = store.latest_tx_id().unwrap();
    store
        .fork_create("fixture", tx, "2026-01-01T00:00:00Z", None)
        .unwrap();
    assert_fact(&mut store, "urn:e", "root-only", TT);
    let file = emitted(&store, &json!({"fork": "fixture"}));
    assert_eq!(file["request"]["base"], "");
    assert_eq!(file["request"]["theirs"], "");
    assert_ne!(file["request"]["ours"], "");
    (store, file)
}

#[test]
fn legitimate_fresh_fork_keeps_a_root_only_addition() {
    authenticated(|| {
        let (mut store, file) = fork_fixture();
        let out = apply(&mut store, &json!({}), &file).unwrap();
        assert_eq!(out["outcome"], "applied", "{out}");
        assert_eq!(
            live_starts(&store, "urn:e"),
            [("\"root-only\"".into(), TT.into())]
        );
    });
}

#[test]
fn fresh_fork_with_a_forged_base_writes_nothing() {
    authenticated(|| {
        let (mut store, mut file) = fork_fixture();
        // Base := ROOT turns the ROOT-only fact into an incoming deletion.
        file["request"]["base"] = file["request"]["ours"].clone();
        refused_without_writes(&mut store, &json!({}), &file);
        assert_eq!(live_starts(&store, "urn:e").len(), 1);
    });
}

fn forged_fork_field_writes_nothing(field: &str) {
    authenticated(|| {
        let (mut store, mut file) = fork_fixture();
        match field {
            "shapes" => file["request"]["shapes"] = json!(""),
            "authored_at" => file["authored_at"] = json!("2020-01-01T00:00:00Z"),
            "active_slots" => file["request"]["active_slots"] = json!([]),
            _ => unreachable!(),
        }
        refused_without_writes(&mut store, &json!({}), &file);
    });
}
#[test]
fn fresh_fork_with_forged_shapes_writes_nothing() {
    forged_fork_field_writes_nothing("shapes");
}
#[test]
fn fresh_fork_with_a_forged_clock_writes_nothing() {
    forged_fork_field_writes_nothing("authored_at");
}
#[test]
fn fresh_fork_with_a_forged_selection_writes_nothing() {
    forged_fork_field_writes_nothing("active_slots");
}

fn inline(dir: &std::path::Path) -> Json {
    let read = |name: &str| std::fs::read_to_string(dir.join(name)).unwrap();
    json!({
        "manifest": serde_json::from_str::<Json>(&read("manifest.json")).unwrap(),
        "export_ntriples": read("export.nt"),
        "shapes_turtle": read("shapes.ttl"),
    })
}
fn seeded() -> Store {
    let mut store = Store::open_in_memory().unwrap();
    crate::share_scrub::seed_test_catalogue(&mut store);
    store
        .load_shapes("fresh", SHAPES, "2026-01-01T00:00:00Z")
        .unwrap();
    assert_fact(&mut store, "urn:e", "shared", "2026-01-02T00:00:00Z");
    store
}

/// Gennaro's P2 case: a clean incoming assertion takes the manifest's clock.
fn share_fixture() -> (tempfile::TempDir, Store, Json, Json, String) {
    let dir = tempfile::tempdir().unwrap();
    let mut source = seeded();
    let share = |store: &Store, name: &str, parent: Option<String>| {
        crate::share::share(
            store,
            dir.path().join(name).to_str().unwrap(),
            &crate::share::ShareOptions {
                parent_share: parent,
                ..Default::default()
            },
        )
        .unwrap()
    };
    let base = share(&source, "base", None);
    assert_fact(&mut source, "urn:f", "incoming", "2026-01-05T00:00:00Z");
    share(&source, "incoming", Some(base.share_id));
    let envelope = json!({
        "incoming": inline(&dir.path().join("incoming")),
        "base": inline(&dir.path().join("base")),
    });
    let created = envelope["incoming"]["manifest"]["created_at"]
        .as_str()
        .unwrap()
        .to_owned();
    let local = seeded();
    let file = emitted(&local, &envelope);
    assert_eq!(file["authored_at"], created.as_str());
    (dir, local, envelope, file, created)
}

#[test]
fn legitimate_fresh_share_uses_the_verified_manifest_clock() {
    authenticated(|| {
        let (_dir, mut store, envelope, file, created) = share_fixture();
        let out = apply(&mut store, &envelope, &file).unwrap();
        assert_eq!(out["outcome"], "applied", "{out}");
        assert_eq!(
            live_starts(&store, "urn:f"),
            [("\"incoming\"".into(), created)]
        );
    });
}

#[test]
fn fresh_share_with_a_forged_clock_writes_nothing() {
    authenticated(|| {
        let (_dir, mut store, envelope, mut file, _) = share_fixture();
        file["authored_at"] = json!("2020-01-01T00:00:00Z");
        refused_without_writes(&mut store, &envelope, &file);
        assert!(live_starts(&store, "urn:f").is_empty());
    });
}
