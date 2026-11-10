//! Q2 crosses the real producer, transport and atomic import boundaries.
use crate::Store;
use crate::competency::carriage::{COMMANDS_FILE, imported_members, verified_projection};
use crate::share::{ShareOptions, SharePayload, manifest_bytes, sha256, share_payload};
use crate::share_import::{ShareImportRequest, import_share, verify_share};
use crate::share_scrub::ShareDestination;
use crate::store::queries::StoredQuery;

const TS: &str = "2026-11-05T00:00:00Z";
const SOURCE: &str = r#"# carried verbatim
schema = "quipu-competency/1"
[[question]]
id = "present"
ask = "Is someone present?"
query_ref = "legacy"
justifies = ["urn:PersonShape"]
expect = "ask=true"
[[command]]
id = "record"
does = "Record a person"
inputs = { person = "iri" }
template = '<{person}> a <urn:Person> .'
satisfies = ["urn:PersonShape"]
write_class = "record-person"
key = ["person"]
example = { person = "urn:alice" }
"#;

fn producer() -> Store {
    let store = Store::open_in_memory().unwrap();
    store
        .query_load(
            &StoredQuery {
                name: "legacy".into(),
                description: "Independent read".into(),
                template: "ASK {}".into(),
                dataset: None,
                params: vec![],
            },
            TS,
        )
        .unwrap();
    store
}
fn payload(source: Option<&str>) -> SharePayload {
    share_payload(
        &producer(),
        &ShareOptions {
            no_shapes: true,
            destination: ShareDestination::Internal,
            competency_toml: source.map(str::to_owned),
            ..Default::default()
        },
        crate::share::SHARE_PAYLOAD_MAX_BYTES,
    )
    .unwrap()
}
fn request(p: &SharePayload) -> ShareImportRequest {
    serde_json::from_value(serde_json::json!({
        "manifest": p.manifest,
        "export_ntriples": p.files["export.nt"],
        "shapes_turtle": p.files["shapes.ttl"],
        "queries_turtle": p.files.get("queries.ttl"),
        "competency_toml": p.files.get("competency.toml"),
        "commands_turtle": p.files.get("commands.ttl"),
        "source": "competency integration fixture"
    }))
    .unwrap()
}
fn reseal(r: &mut ShareImportRequest) {
    r.manifest.competency_hash = r.competency_toml.as_ref().map(|v| sha256(v.as_bytes()));
    r.manifest.commands_hash = r.commands_turtle.as_ref().map(|v| sha256(v.as_bytes()));
    r.manifest.queries_hash = r.queries_turtle.as_ref().map(|v| sha256(v.as_bytes()));
    r.manifest.share_id = sha256(&manifest_bytes(&r.manifest, false).unwrap());
}

#[test]
fn full_extended_contract_is_verified_before_plain_query_projection_and_staging() {
    let p = payload(Some(SOURCE));
    assert_eq!(p.files["competency.toml"], SOURCE);
    assert!(p.files["queries.ttl"].contains("quipu:justifies"));
    // The legacy parser remains closed: accepting the full RDF directly would
    // bypass its stored-query-only contract. This assertion must stay red if
    // somebody "fixes" integration by globally relaxing it.
    assert!(crate::share_queries::from_turtle(&p.files["queries.ttl"]).is_err());
    let r = request(&p);
    verify_share(&r).unwrap();
    let plain = verified_projection(&r).unwrap().unwrap();
    let queries = crate::share_queries::from_turtle(&plain).unwrap();
    assert_eq!(queries.len(), 1);
    assert_eq!(queries[0].query.name, "legacy");
    // Competency questions and commands stay inert contract data. They are not
    // silently registered as unconstrained legacy executable queries.
    let mut receiver = Store::open_in_memory().unwrap();
    let result = import_share(&mut receiver, &r, TS, None).unwrap();
    assert_eq!(result.outcome, "staged");
    assert_eq!(result.queries.unwrap().awaiting_promotion.len(), 1);
    assert!(receiver.query_list().unwrap().is_empty());
    let retained = imported_members(&receiver, &r.manifest.share_id)
        .unwrap()
        .unwrap();
    assert_eq!(retained.authoring, SOURCE);
    assert_eq!(retained.queries, p.files["queries.ttl"]);
    assert_eq!(retained.commands, p.files[COMMANDS_FILE]);
}

#[test]
fn a_resealed_full_contract_mismatch_refuses_before_projection_or_any_import_write() {
    let original = request(&payload(Some(SOURCE)));
    for case in 0..4 {
        let mut r = original.clone();
        match case {
            0 => {
                r.competency_toml =
                    Some(SOURCE.replace("Is someone present?", "Is anything present?"));
            }
            1 => {
                r.commands_turtle = Some(
                    r.commands_turtle
                        .unwrap()
                        .replace("record-person", "forged-class"),
                );
            }
            2 => {
                r.queries_turtle = Some(r.queries_turtle.unwrap().replace("ask=true", "ask=false"));
            }
            _ => {
                r.queries_turtle = Some(format!(
                    "{}\n<urn:smuggled> <urn:p> <urn:o> .\n",
                    r.queries_turtle.unwrap()
                ));
            }
        }
        reseal(&mut r); // Malicious producer can recompute every public digest.
        assert!(verified_projection(&r).is_err(), "case {case}");
        let mut receiver = Store::open_in_memory().unwrap();
        let before = receiver.latest_tx_id().unwrap();
        assert!(
            import_share(&mut receiver, &r, TS, None).is_err(),
            "case {case}"
        );
        assert_eq!(receiver.latest_tx_id().unwrap(), before);
        assert!(receiver.query_list().unwrap().is_empty());
        assert!(
            imported_members(&receiver, &r.manifest.share_id)
                .unwrap()
                .is_none()
        );
    }
}

#[test]
fn partial_misnamed_undeclared_or_changed_members_refuse() {
    let original = request(&payload(Some(SOURCE)));
    for case in 0..8 {
        let mut r = original.clone();
        match case {
            0 => r.competency_toml = None,
            1 => r.commands_turtle = None,
            2 => r.queries_turtle = None,
            3 => r.manifest.files.competency = Some("../competency.toml".into()),
            4 => r.manifest.files.commands = None,
            5 => r.manifest.competency_hash = None,
            6 => r.manifest.commands_hash = None,
            _ => r.commands_turtle.as_mut().unwrap().push(' '),
        }
        assert!(verify_share(&r).is_err(), "case {case}");
    }
    let mut undeclared = request(&payload(None));
    undeclared.commands_turtle = Some("# undeclared".into());
    assert!(verify_share(&undeclared).is_err());
}

#[test]
fn no_contract_omits_all_new_manifest_fields_and_preserves_legacy_verification() {
    let p = payload(None);
    let value = serde_json::to_value(&p.manifest).unwrap();
    for key in ["competency_hash", "commands_hash"] {
        assert!(value.get(key).is_none());
    }
    for key in ["competency", "commands"] {
        assert!(value["files"].get(key).is_none());
    }
    assert!(!p.files.contains_key("competency.toml"));
    assert!(!p.files.contains_key(COMMANDS_FILE));
    let r = request(&p);
    assert!(verified_projection(&r).unwrap().is_none());
    verify_share(&r).unwrap();
}

#[test]
fn directory_and_archive_carry_all_sealed_members_and_merge_paths_refuse() {
    let p = payload(Some(SOURCE));
    let dir = tempfile::tempdir().unwrap();
    for (name, bytes) in &p.files {
        std::fs::write(dir.path().join(name), bytes).unwrap();
    }
    let r = crate::share_transport::read_local(dir.path().to_str().unwrap()).unwrap();
    verify_share(&r).unwrap();
    assert_eq!(r.competency_toml.as_deref(), Some(SOURCE));
    let mut archive = tar::Builder::new(Vec::new());
    for (name, bytes) in &p.files {
        let mut header = tar::Header::new_gnu();
        header.set_size(bytes.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        archive
            .append_data(&mut header, name, bytes.as_bytes())
            .unwrap();
    }
    let bytes = archive.into_inner().unwrap();
    let artifact = dir.path().join("fixture.pendant.tar");
    std::fs::write(&artifact, bytes).unwrap();
    let carried = crate::share_transport::read_local(artifact.to_str().unwrap()).unwrap();
    verify_share(&carried).unwrap();
    assert_eq!(carried.commands_turtle, r.commands_turtle);
    assert!(crate::share_merge::read_share(dir.path()).is_err());
}

#[test]
#[cfg(not(target_arch = "wasm32"))]
fn full_store_reconstruction_preserves_verified_contract_bytes() {
    let r = request(&payload(Some(SOURCE)));
    let mut receiver = Store::open_in_memory().unwrap();
    import_share(&mut receiver, &r, TS, None).unwrap();
    let before = imported_members(&receiver, &r.manifest.share_id)
        .unwrap()
        .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("full.pendant.db");
    crate::pack_full::pack_full(
        &receiver,
        out.to_str().unwrap(),
        &crate::pack::PackOptions {
            destination: ShareDestination::Internal,
            ..Default::default()
        },
        TS,
    )
    .unwrap();
    let restored = Store::open(out.to_str().unwrap()).unwrap();
    let after = imported_members(&restored, &r.manifest.share_id)
        .unwrap()
        .unwrap();
    assert_eq!(before, after);
}
