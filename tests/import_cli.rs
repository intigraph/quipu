//! Archive imports must use the operator's database without trusting pack shapes.
#![cfg(feature = "shacl")]

use std::path::Path;
use std::process::Command;

const SHAPES: &str = "@prefix sh: <http://www.w3.org/ns/shacl#> .\n<urn:WidgetShape> a sh:NodeShape; sh:targetClass <urn:Widget> .";

fn cli(root: &Path, args: &[&str]) -> serde_json::Value {
    let output = Command::new(env!("CARGO_BIN_EXE_quipu"))
        .current_dir(root)
        .env("HOME", root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn fixture(root: &Path) -> (String, String) {
    let mut source = quipu::Store::open_in_memory().unwrap();
    source.load_shapes("fixture", SHAPES, "2026-09-11").unwrap();
    quipu::ingest_rdf(
        &mut source,
        &b"<urn:item> a <urn:Widget> ."[..],
        oxrdfio::RdfFormat::Turtle,
        None,
        "2026-09-11",
        None,
        None,
    )
    .unwrap();
    let policy_graph = source.overlay_create("urn:test:catalogue", 0).unwrap();
    quipu::rdf::ingest_rdf_to_graph(
        &mut source,
        include_bytes!("fixtures/share-catalogue.ttl").as_slice(),
        oxrdfio::RdfFormat::Turtle,
        None,
        "2026-09-11",
        None,
        Some("import-cli-policy"),
        policy_graph,
    )
    .unwrap();
    let dir = root.join("share");
    quipu::share::share(&source, dir.to_str().unwrap(), &Default::default()).unwrap();
    let archive = root.join("share.pendant.tar.gz");
    let zip = flate2::write::GzEncoder::new(
        std::fs::File::create(&archive).unwrap(),
        flate2::Compression::default(),
    );
    let mut tar = tar::Builder::new(zip);
    for name in ["manifest.json", "export.nt", "shapes.ttl"] {
        tar.append_path_with_name(dir.join(name), name).unwrap();
    }
    tar.into_inner().unwrap().finish().unwrap();
    (
        dir.to_str().unwrap().into(),
        archive.to_str().unwrap().into(),
    )
}

#[test]
fn directory_and_archive_stage_in_the_selected_database_and_promote_after_reopen() {
    let root = tempfile::tempdir().unwrap();
    let (dir, archive) = fixture(root.path());
    for (index, reference) in [dir, archive].iter().enumerate() {
        let db = root.path().join(format!("receiver-{index}.db"));
        let mut receiver = quipu::Store::open(db.to_str().unwrap()).unwrap();
        receiver
            .load_shapes("fixture", SHAPES, "2026-09-11")
            .unwrap();
        drop(receiver);
        let imported = cli(
            root.path(),
            &["import", reference, "--db", db.to_str().unwrap()],
        );
        assert_eq!(imported["outcome"], "staged", "{imported}");
        assert_eq!(imported["triples"]["accepted"], 1);
        assert_eq!(
            imported["validation"]["off_vocabulary"],
            serde_json::json!([])
        );
        let promoted = cli(
            root.path(),
            &[
                "import",
                "promote",
                imported["share_id"].as_str().unwrap(),
                "--db",
                db.to_str().unwrap(),
            ],
        );
        assert_eq!(promoted["outcome"], "promoted", "{promoted}");
        assert_eq!(promoted["triples"], 1);
    }
}

#[test]
fn archive_does_not_adopt_carried_shapes_or_create_a_default_database() {
    let root = tempfile::tempdir().unwrap();
    let (_, archive) = fixture(root.path());
    let transient = cli(root.path(), &["import", &archive]);
    assert_eq!(transient["outcome"], "quarantined");
    assert!(!root.path().join(".bobbin").exists());
    let db = root.path().join("empty.db");
    let persisted = cli(
        root.path(),
        &["import", &archive, "--db", db.to_str().unwrap()],
    );
    assert_eq!(persisted["outcome"], "quarantined");
    assert_eq!(
        persisted["validation"]["off_vocabulary"],
        serde_json::json!(["urn:Widget"])
    );
    assert!(db.exists(), "an explicit --db must not be silently ignored");
}

// THE RENAME ALIAS (aegis-fxpbys.3). A pendant is the artifact once called a
// qpack. An archive still named `.qpack.tar.gz` must import exactly as before
// -- same share, same outcome -- and say on stderr what to rename it to; a
// `.pendant.tar.gz` must import without that notice.
#[test]
fn a_legacy_qpack_archive_still_imports_and_warns_while_a_pendant_does_not() {
    let root = tempfile::tempdir().unwrap();
    let (_, pendant) = fixture(root.path());
    let legacy = root.path().join("share.qpack.tar.gz");
    std::fs::copy(&pendant, &legacy).unwrap();
    let run = |reference: &str| {
        let output = Command::new(env!("CARGO_BIN_EXE_quipu"))
            .current_dir(root.path())
            .env("HOME", root.path())
            .args(["import", reference])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        (json, String::from_utf8_lossy(&output.stderr).into_owned())
    };
    let (new_json, new_err) = run(&pendant);
    let (old_json, old_err) = run(legacy.to_str().unwrap());
    assert_eq!(old_json["share_id"], new_json["share_id"]);
    assert_eq!(old_json["outcome"], new_json["outcome"]);
    assert!(
        old_err.contains("deprecated") && old_err.contains("share.pendant.tar.gz"),
        "legacy name must warn: {old_err}"
    );
    assert!(
        !new_err.contains("deprecated"),
        "a pendant must not warn: {new_err}"
    );
}

// aegis-zev (review of #954): `[quipu.import] refuse_internal` must govern every
// import arm alike. A transient archive or URL import opens its own in-memory
// store, and that store was built without the operator's import policy, so a
// strict receiver refused the directory but staged the same share as an archive.

/// Run the binary with `root` as cwd and HOME (so `.bobbin/config.toml` there is
/// the only config read), returning the raw output.
fn run(root: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_quipu"))
        .current_dir(root)
        .env("HOME", root)
        .args(args)
        .output()
        .unwrap()
}

/// An internal (stamped) share as a directory and as an archive under `root`.
fn internal_fixture(root: &Path) -> (String, String) {
    let mut source = quipu::Store::open_in_memory().unwrap();
    quipu::ingest_rdf(
        &mut source,
        &b"<urn:item> <urn:p> \"kept internal\" ."[..],
        oxrdfio::RdfFormat::Turtle,
        None,
        "2026-10-31",
        None,
        None,
    )
    .unwrap();
    let dir = root.join("internal-share");
    let manifest = quipu::share::share(
        &source,
        dir.to_str().unwrap(),
        &quipu::share::ShareOptions {
            no_shapes: true,
            destination: quipu::share::ShareDestination::Internal,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        manifest.destination,
        Some(quipu::share::ShareDestination::Internal),
        "fixture must be stamped, or every arm below is untested"
    );
    let archive = root.join("internal.pendant.tar.gz");
    let zip = flate2::write::GzEncoder::new(
        std::fs::File::create(&archive).unwrap(),
        flate2::Compression::default(),
    );
    let mut tar = tar::Builder::new(zip);
    for name in ["manifest.json", "export.nt", "shapes.ttl"] {
        tar.append_path_with_name(dir.join(name), name).unwrap();
    }
    tar.into_inner().unwrap().finish().unwrap();
    (
        dir.to_str().unwrap().into(),
        archive.to_str().unwrap().into(),
    )
}

fn strict(root: &Path) {
    std::fs::create_dir_all(root.join(".bobbin")).unwrap();
    std::fs::write(
        root.join(".bobbin/config.toml"),
        "[quipu.import]\nrefuse_internal = true\n",
    )
    .unwrap();
}

fn assert_accepted(output: &std::process::Output, arm: &str) {
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{arm}: {stderr}");
    assert!(
        stderr.contains("internal share: unscrubbed identifiers, kept internal"),
        "{arm}: {stderr}"
    );
    let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["destination"], "internal", "{arm}: {result}");
}

fn assert_refused(output: &std::process::Output, arm: &str) {
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success(),
        "{arm} staged under refuse_internal"
    );
    assert!(stderr.contains("refuse_internal"), "{arm}: {stderr}");
    assert!(output.stdout.is_empty(), "{arm} printed a result");
}

#[test]
fn every_import_arm_accepts_an_internal_share_by_default() {
    let root = tempfile::tempdir().unwrap();
    let (dir, archive) = internal_fixture(root.path());
    let db = root.path().join("receiver.db");
    let db = db.to_str().unwrap();
    assert_accepted(
        &run(root.path(), &["import", &dir, "--db", db]),
        "directory",
    );
    assert_accepted(
        &run(root.path(), &["import", &archive]),
        "transient archive",
    );
    assert_accepted(
        &run(root.path(), &["import", &archive, "--db", db]),
        "archive with --db",
    );
}

#[test]
fn refuse_internal_refuses_every_import_arm_alike() {
    let root = tempfile::tempdir().unwrap();
    let (dir, archive) = internal_fixture(root.path());
    strict(root.path());
    let db = root.path().join("receiver.db");
    let db = db.to_str().unwrap();
    assert_refused(
        &run(root.path(), &["import", &dir, "--db", db]),
        "directory",
    );
    assert_refused(
        &run(root.path(), &["import", &archive]),
        "transient archive",
    );
    assert_refused(
        &run(root.path(), &["import", &archive, "--db", db]),
        "archive with --db",
    );
}

/// Serve `body` for every request on a loopback port; returns the base URL.
#[cfg(feature = "remote")]
fn serve(body: Vec<u8>) -> String {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut request = [0_u8; 4096];
            let _ = stream.read(&mut request);
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/gzip\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.write_all(&body);
        }
    });
    format!("http://127.0.0.1:{port}")
}

#[cfg(feature = "remote")]
#[test]
fn a_url_import_follows_the_same_internal_policy() {
    let root = tempfile::tempdir().unwrap();
    let (_, archive) = internal_fixture(root.path());
    let url = format!(
        "{}/internal.pendant.tar.gz",
        serve(std::fs::read(&archive).unwrap())
    );
    assert_accepted(&run(root.path(), &["import", &url]), "URL default");
    strict(root.path());
    assert_refused(&run(root.path(), &["import", &url]), "URL strict");
}
