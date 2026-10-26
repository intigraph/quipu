use super::*;
use crate::entity_namespaces::{Namespace, apply, configure};
use crate::entity_reference::tool_entity_ref;
use serde_json::json;

#[test]
fn concurrent_allocation_cannot_race_a_shape_contract_edit() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("identity.db");
    let mut store = Store::open(path.to_str().unwrap()).unwrap();
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
    // The registry edit's guard runs under the mapping settlement's writer
    // reservation (BEGIN IMMEDIATE), so a key allocation cannot interleave
    // between the identity check and the shape contract edit.
    let mut raced = None;
    let replayed = store
        .mapping_atomic(
            "urn:quipu:principal:test",
            |store| {
                crate::entity_key_guard::check_shapes(store, &candidate)?;
                raced = Some(tool_entity_ref(
                    &mut writer,
                    &json!({"action":"mint","namespace":"ep","type":ty,"keys":{host:"db.example"}}),
                ));
                Ok(crate::store::mapping_atomic::Preparation::<(), ()>::Replay(()))
            },
            |_, (), _, _| unreachable!("a replay preparation never writes"),
        )
        .unwrap();
    assert_eq!(replayed, ());
    assert!(
        matches!(raced, Some(Err(crate::Error::Sqlite(rusqlite::Error::SqliteFailure(error, _))))
            if error.code == rusqlite::ErrorCode::DatabaseBusy),
        "an allocation must not commit inside the shape edit's reservation: {raced:?}"
    );
    assert!(
        store.conn.is_autocommit(),
        "the settlement must release its reservation"
    );
    let allocated = tool_entity_ref(
        &mut writer,
        &json!({"action":"mint","namespace":"ep","type":ty,"keys":{host:"db.example"}}),
    )
    .unwrap();
    assert_eq!(store.get_combined_shapes().unwrap().unwrap(), shape);
    let result = tool_entity_ref(
        &mut writer,
        &json!({"action":"verify","iri":allocated["iri"]}),
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
