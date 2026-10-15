use super::*;
use crate::store::Datum;
use crate::types::Op;
use crate::{Store, Value};
use base64::{Engine, engine::general_purpose::STANDARD};

fn request(query: &str) -> RenderRequest {
    RenderRequest {
        spec: None,
        adhoc: Some(RenderSpec {
            query: query.into(),
            format: "markdown".into(),
            layout: "single".into(),
            path: "report.md".into(),
            template: Some("{% for row in rows %}{{ row.content }}{% endfor %}".into()),
            includes: vec![],
            provenance: false,
        }),
        valid_at: None,
        graph: None,
    }
}
fn assert_fact(store: &mut Store, subject: &str, predicate: &str, value: Value, ts: &str) {
    let e = store.intern(subject).unwrap();
    let a = store.intern(predicate).unwrap();
    store
        .transact(
            &[Datum {
                entity: e,
                attribute: a,
                value,
                valid_from: ts.into(),
                valid_to: None,
                op: Op::Assert,
            }],
            ts,
            None,
            Some("render-fixture"),
        )
        .unwrap();
}

#[test]
fn raw_markdown_and_deterministic_noop_have_no_writes() {
    let store = Store::open_in_memory().unwrap();
    let req = request(
        "SELECT ?content WHERE { VALUES ?content { '---\\nname: literal\\n---\\nbytes\\n' } }",
    );
    let plan = prepare(&store, &req).unwrap();
    assert_eq!(plan, prepare(&store, &req).unwrap());
    assert_eq!(
        STANDARD.decode(&plan.files["report.md"].base64).unwrap(),
        b"---\nname: literal\n---\nbytes\n"
    );
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("owned");
        assert!(!publish::publish(&plan, &dest, true).unwrap().noop);
        assert!(!dest.exists());
        publish::publish(&plan, &dest, false).unwrap();
        let before = std::fs::read(dest.join(".quipu-render.json")).unwrap();
        let mt = std::fs::metadata(dest.join("report.md"))
            .unwrap()
            .modified()
            .unwrap();
        assert!(publish::publish(&plan, &dest, false).unwrap().noop);
        assert_eq!(
            before,
            std::fs::read(dest.join(".quipu-render.json")).unwrap()
        );
        assert_eq!(
            mt,
            std::fs::metadata(dest.join("report.md"))
                .unwrap()
                .modified()
                .unwrap()
        );
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }
}

#[test]
fn stored_spec_uses_same_temporal_view_and_rejects_duplicate_fields() {
    let mut store = Store::open_in_memory().unwrap();
    let typ = store.intern(SPEC_TYPE).unwrap();
    assert_fact(
        &mut store,
        "urn:spec",
        "http://www.w3.org/1999/02/22-rdf-syntax-ns#type",
        Value::Ref(typ),
        "2026-01-01T00:00:00Z",
    );
    assert_fact(
        &mut store,
        "urn:spec",
        &format!("{NS}query"),
        Value::Str("SELECT ?content WHERE { <urn:doc> <urn:text> ?content }".into()),
        "2026-01-01T00:00:00Z",
    );
    assert_fact(
        &mut store,
        "urn:spec",
        &format!("{NS}template"),
        Value::Str("{{ rows[0].content }}".into()),
        "2026-01-01T00:00:00Z",
    );
    assert_fact(
        &mut store,
        "urn:doc",
        "urn:text",
        Value::Str("old".into()),
        "2026-01-01T00:00:00Z",
    );
    let e = store.intern("urn:doc").unwrap();
    let a = store.intern("urn:text").unwrap();
    store
        .transact(
            &[
                Datum {
                    entity: e,
                    attribute: a,
                    value: Value::Str("old".into()),
                    valid_from: "2026-02-01T00:00:00Z".into(),
                    valid_to: None,
                    op: Op::Retract,
                },
                Datum {
                    entity: e,
                    attribute: a,
                    value: Value::Str("new".into()),
                    valid_from: "2026-02-01T00:00:00Z".into(),
                    valid_to: None,
                    op: Op::Assert,
                },
            ],
            "2026-02-01T00:00:00Z",
            None,
            None,
        )
        .unwrap();
    let mut req = RenderRequest {
        spec: Some("urn:spec".into()),
        adhoc: None,
        valid_at: Some("2026-01-15T00:00:00Z".into()),
        graph: None,
    };
    assert_eq!(
        STANDARD
            .decode(&prepare(&store, &req).unwrap().files["report.md"].base64)
            .unwrap(),
        b"old"
    );
    req.valid_at = None;
    assert_eq!(
        STANDARD
            .decode(&prepare(&store, &req).unwrap().files["report.md"].base64)
            .unwrap(),
        b"new"
    );
    assert_fact(
        &mut store,
        "urn:spec",
        &format!("{NS}template"),
        Value::Str("different".into()),
        "2026-03-01T00:00:00Z",
    );
    assert!(
        prepare(&store, &req)
            .unwrap_err()
            .to_string()
            .contains("duplicate")
    );
}

#[test]
fn hierarchy_includes_modes_hashes_and_conflicts() {
    let store = Store::open_in_memory().unwrap();
    let mut req = request(
        "SELECT ?slug ?content WHERE { VALUES (?slug ?content) { ('first' 'one') ('second' 'two') } }",
    );
    let spec = req.adhoc.as_mut().unwrap();
    spec.layout = "per-entity".into();
    spec.path = "{slug}/SKILL.md".into();
    let bytes = b"#!/bin/sh\necho fixture\n";
    spec.includes.push(Include {
        path: "{slug}/scripts/run.sh".into(),
        sha256: crate::share::sha256(bytes),
        base64: STANDARD.encode(bytes),
        mode: 0o755,
    });
    let plan = prepare(&store, &req).unwrap();
    assert_eq!(plan.files.len(), 4);
    assert_eq!(plan.files["first/scripts/run.sh"].mode, 0o755);
    req.adhoc.as_mut().unwrap().includes[0].sha256 = "bad".into();
    assert!(
        prepare(&store, &req)
            .unwrap_err()
            .to_string()
            .contains("hash")
    );
    req.adhoc.as_mut().unwrap().includes.clear();
    req.adhoc.as_mut().unwrap().path = "same.md".into();
    assert!(
        prepare(&store, &req)
            .unwrap_err()
            .to_string()
            .contains("colliding")
    );
}

#[test]
fn query_unknown_and_template_escape_never_produce_plans() {
    let store = Store::open_in_memory().unwrap();
    for query in [
        "ASK {}",
        "SELECT ?x WHERE { BIND(NOW() AS ?x) }",
        "SELECT * WHERE { SERVICE <https://example.org> {?s ?p ?o} }",
    ] {
        assert!(prepare(&store, &request(query)).is_err());
    }
    for template in [
        "{% include '/etc/passwd' %}",
        "{{ read_file('/etc/passwd') }}",
        "{{ range(1000000000) }}",
        "{{ unknown }}",
    ] {
        let mut req = request("SELECT ?content WHERE { VALUES ?content {'ok'} }");
        req.adhoc.as_mut().unwrap().template = Some(template.into());
        assert!(prepare(&store, &req).is_err());
    }
    for path in [
        "../escape",
        "/absolute",
        "a/../../escape",
        "a\\b",
        ".quipu-render.json",
        "a//b",
        "a:stream",
    ] {
        assert!(check_path(path).is_err());
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn publication_refuses_unmanaged_edited_symlink_and_stale_mode() {
    let store = Store::open_in_memory().unwrap();
    let plan = prepare(
        &store,
        &request("SELECT ?content WHERE {VALUES ?content {'ok'}}"),
    )
    .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("owned");
    std::fs::create_dir(&dest).unwrap();
    std::fs::write(dest.join("unknown"), b"personal").unwrap();
    assert!(publish::publish(&plan, &dest, false).is_err());
    assert_eq!(std::fs::read(dest.join("unknown")).unwrap(), b"personal");
    std::fs::remove_file(dest.join("unknown")).unwrap();
    publish::publish(&plan, &dest, false).unwrap();
    std::fs::write(dest.join("report.md"), b"edited").unwrap();
    assert!(publish::publish(&plan, &dest, false).is_err());
    assert_eq!(std::fs::read(dest.join("report.md")).unwrap(), b"edited");
    std::fs::remove_dir_all(&dest).unwrap();
    std::os::unix::fs::symlink(dir.path(), &dest).unwrap();
    assert!(publish::publish(&plan, &dest, false).is_err());
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn changed_plan_preserves_unaffected_inode_and_deletes_only_owned_file() {
    use std::os::unix::fs::MetadataExt;
    let store = Store::open_in_memory().unwrap();
    let mut req = request("SELECT ?content WHERE {VALUES ?content {'ok'}}");
    req.adhoc.as_mut().unwrap().includes.push(Include {
        path: "include.txt".into(),
        sha256: crate::share::sha256(b"stable"),
        base64: STANDARD.encode(b"stable"),
        mode: 0o644,
    });
    let first = prepare(&store, &req).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("owned");
    publish::publish(&first, &dest, false).unwrap();
    let inode = std::fs::metadata(dest.join("include.txt")).unwrap().ino();
    req.adhoc.as_mut().unwrap().template = Some("changed".into());
    let second = prepare(&store, &req).unwrap();
    assert_eq!(
        publish::publish(&second, &dest, false).unwrap().changed,
        vec!["report.md"]
    );
    assert_eq!(
        std::fs::metadata(dest.join("include.txt")).unwrap().ino(),
        inode
    );
    req.adhoc.as_mut().unwrap().includes.clear();
    let third = prepare(&store, &req).unwrap();
    assert_eq!(
        publish::publish(&third, &dest, false).unwrap().removed,
        vec!["include.txt"]
    );
    assert!(!dest.join("include.txt").exists());
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn committed_mid_render_write_cannot_change_present_or_historical_snapshot() {
    for valid_at in [None, Some("2026-03-01T00:00:00Z".to_string())] {
        let root = tempfile::tempdir().unwrap();
        let database = root.path().join("fixture.db");
        let mut reader = Store::open(database.to_str().unwrap()).unwrap();
        assert_fact(
            &mut reader,
            "urn:doc",
            "urn:text",
            Value::Str("old".into()),
            "2026-01-01T00:00:00Z",
        );
        let before_tx = reader.latest_tx_id().unwrap();
        let mut writer = Store::open(database.to_str().unwrap()).unwrap();
        let mut req = request("SELECT ?content WHERE { <urn:doc> <urn:text> ?content }");
        req.valid_at = valid_at.clone();
        let plan = plan::prepare_with_interleave(&reader, &req, || {
            let e = writer.intern("urn:doc")?;
            let a = writer.intern("urn:text")?;
            writer.transact(
                &[
                    Datum {
                        entity: e,
                        attribute: a,
                        value: Value::Str("old".into()),
                        valid_from: "2026-02-01T00:00:00Z".into(),
                        valid_to: None,
                        op: Op::Retract,
                    },
                    Datum {
                        entity: e,
                        attribute: a,
                        value: Value::Str("new".into()),
                        valid_from: "2026-02-01T00:00:00Z".into(),
                        valid_to: None,
                        op: Op::Assert,
                    },
                ],
                "2026-02-01T00:00:00Z",
                None,
                Some("concurrent-writer"),
            )?;
            assert!(writer.latest_tx_id()? > before_tx);
            Ok(())
        })
        .unwrap();
        assert_eq!(plan.source_tx, before_tx);
        assert_eq!(
            STANDARD.decode(&plan.files["report.md"].base64).unwrap(),
            b"old"
        );
        let dest = root.path().join("rendered");
        publish::publish(&plan, &dest, false).unwrap();
        let manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(dest.join(".quipu-render.json")).unwrap())
                .unwrap();
        assert_eq!(manifest["source_tx"], before_tx);
        assert_eq!(manifest["valid_at"], serde_json::json!(valid_at));
        let later = prepare(&reader, &req).unwrap();
        assert!(later.source_tx > before_tx);
        assert_eq!(
            STANDARD.decode(&later.files["report.md"].base64).unwrap(),
            b"new"
        );
    }
}
