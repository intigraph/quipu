use super::*;
use crate::entity_namespaces::{Namespace, apply, configure};
use crate::entity_reference::tool_entity_ref;
use serde_json::json;

#[test]
fn concurrent_allocation_cannot_race_a_shape_contract_edit() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("identity.db");
    let store = Store::open(path.to_str().unwrap()).unwrap();
    let mut writer = Store::open(path.to_str().unwrap()).unwrap();
    let ty = "urn:example:Endpoint";
    let host = "urn:example:host";
    let shape = format!(
        "@prefix sh: <http://www.w3.org/ns/shacl#> .
         @prefix q: <https://quipu.dev/ontology/> .
         <urn:example:Shape> a sh:NodeShape; sh:targetClass <{ty}>;
         q:keyFields (<{host}>); q:keyStrategy \"hash\";
         sh:property [sh:path <{host}>; sh:minCount 1; sh:maxCount 1;
           sh:datatype <http://www.w3.org/2001/XMLSchema#string>]."
    );
    let mut config = crate::QuipuConfig::default();
    config.namespaces.push(Namespace {
        prefix: "ep".into(),
        base_iri: "https://example.org/endpoints/".into(),
        minting: "key".into(),
        types: vec![ty.into()],
        ..Namespace::default()
    });
    configure(&mut writer, &config).unwrap();
    apply(&mut writer).unwrap();
    store
        .load_shapes("endpoint", &shape, &crate::time::now_iso())
        .unwrap();
    let candidate = shape.replace("\"hash\"", "\"lexical\"");
    let mut allocated = None;
    let result = store.guard_shape_edit(|| {
        // Establish the no-allocation snapshot before the other connection mints.
        crate::entity_key_guard::check_shapes(&store, &candidate)?;
        allocated = Some(tool_entity_ref(
            &mut writer,
            &json!({"action":"mint","namespace":"ep","type":ty,"keys":{host:"db.example"}}),
        )?);
        store.load_shapes_checked("endpoint", &candidate, &crate::time::now_iso())
    });
    assert!(
        matches!(result, Err(crate::Error::Sqlite(rusqlite::Error::SqliteFailure(error, _)))
            if error.code == rusqlite::ErrorCode::DatabaseBusy),
        "a stale read snapshot must not authorize a write"
    );
    assert!(
        store.conn.is_autocommit(),
        "refusal must release its snapshot"
    );
    assert_eq!(store.get_combined_shapes().unwrap().unwrap(), shape);
    let result = tool_entity_ref(
        &mut writer,
        &json!({"action":"verify","iri":allocated.unwrap()["iri"]}),
    )
    .unwrap();
    assert_eq!(result["verified"], true);
    // Once a fresh snapshot sees the allocation, the ordinary API refuses too.
    assert!(
        store
            .load_shapes("endpoint", &candidate, &crate::time::now_iso())
            .is_err()
    );
    store
        .load_shapes(
            "unrelated",
            "# independent shape set",
            &crate::time::now_iso(),
        )
        .unwrap();
}
