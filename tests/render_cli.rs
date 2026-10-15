//! Actual CLI, stored specification and portable byte-include contract.
#![cfg(all(feature = "shacl", any(target_os = "linux", target_os = "macos")))]

use base64::{Engine, engine::general_purpose::STANDARD};
use std::path::Path;
use std::process::{Command, Output};

fn cli(root: &Path, db: &Path, args: &[&str]) -> Output {
    let home = root.join("home");
    std::fs::create_dir_all(&home).unwrap();
    Command::new(env!("CARGO_BIN_EXE_quipu"))
        .args(args)
        .args(["--db", db.to_str().unwrap()])
        .current_dir(root)
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .output()
        .unwrap()
}
fn good(result: Output) -> String {
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    String::from_utf8(result.stdout).unwrap()
}

#[test]
fn actual_cli_ad_hoc_dry_run_and_unknown_flag() {
    let root = tempfile::tempdir().unwrap();
    let db = root.path().join("private.db");
    let destination = root.path().join("rendered");
    let args = [
        "render",
        "--query",
        "SELECT ?title WHERE { VALUES ?title { 'Example' } }",
        "--template",
        "# {{ rows[0].title }}",
        "--dest",
        destination.to_str().unwrap(),
        "--dry-run",
    ];
    let planned: serde_json::Value =
        serde_json::from_str(&good(cli(root.path(), &db, &args))).unwrap();
    assert!(planned["dry_run"].as_bool().unwrap());
    assert_eq!(planned["files"], serde_json::json!(["report.md"]));
    assert!(!destination.exists());
    good(cli(root.path(), &db, &args[..args.len() - 1]));
    assert_eq!(
        std::fs::read(destination.join("report.md")).unwrap(),
        b"# Example"
    );
    let before = std::fs::read(destination.join(".quipu-render.json")).unwrap();
    let failed = cli(
        root.path(),
        &db,
        &[
            "render",
            "--query",
            "SELECT ?s WHERE {?s ?p ?o}",
            "--dest",
            destination.to_str().unwrap(),
            "--unknown",
        ],
    );
    assert!(!failed.status.success());
    assert_eq!(
        before,
        std::fs::read(destination.join(".quipu-render.json")).unwrap()
    );
}

#[test]
fn stored_cli_spec_and_includes_round_trip_in_private_pendant() {
    let root = tempfile::tempdir().unwrap();
    let db = root.path().join("private.db");
    let content = "---\r\nname: exact\r\n---\r\nRaw bytes.\r\n";
    let script = b"#!/bin/sh\necho example\n";
    let hash = format!(
        "sha256:{}",
        hex::encode(ring::digest::digest(&ring::digest::SHA256, script).as_ref())
    );
    let includes = serde_json::json!([{"path":"{slug}/scripts/run.sh","sha256":hash,"base64":STANDARD.encode(script),"mode":493}]).to_string();
    let query = "SELECT ?slug ?content ?includes WHERE { <urn:example:skill> <urn:content> ?content ; <urn:includes> ?includes . BIND('example' AS ?slug) }";
    let literal = |value: &str| serde_json::to_string(value).unwrap();
    let rdf = format!(
        "@prefix r: <urn:quipu:render:> .\n<urn:example:spec> a r:RenderSpec ; r:query {} ; r:layout \"per-entity\" ; r:path \"{{slug}}/SKILL.md\" ; r:template \"{{{{ row.content }}}}\" .\n<urn:example:skill> <urn:content> {} ; <urn:includes> {} .\n",
        literal(query),
        literal(content),
        literal(&includes)
    );
    let fixture = root.path().join("spec.ttl");
    std::fs::write(&fixture, rdf).unwrap();
    good(cli(
        root.path(),
        &db,
        &[
            "knot",
            fixture.to_str().unwrap(),
            "--shapes",
            concat!(env!("CARGO_MANIFEST_DIR"), "/shapes/render.ttl"),
        ],
    ));
    let dest = root.path().join("rendered");
    let args = [
        "render",
        "urn:example:spec",
        "--dest",
        dest.to_str().unwrap(),
    ];
    good(cli(root.path(), &db, &args));
    assert_eq!(
        std::fs::read(dest.join("example/SKILL.md")).unwrap(),
        content.as_bytes()
    );
    assert_eq!(
        std::fs::read(dest.join("example/scripts/run.sh")).unwrap(),
        script
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(dest.join("example/scripts/run.sh"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o755
        );
    }
    let manifest = std::fs::read(dest.join(".quipu-render.json")).unwrap();
    let rerun: serde_json::Value =
        serde_json::from_str(&good(cli(root.path(), &db, &args))).unwrap();
    assert_eq!(rerun["diff"]["noop"], true);
    assert_eq!(
        manifest,
        std::fs::read(dest.join(".quipu-render.json")).unwrap()
    );
    let pack = root.path().join("spec.pendant.db");
    // This is a private local fixture, never a public outward export.
    good(cli(
        root.path(),
        &db,
        &[
            "pack",
            "--out",
            pack.to_str().unwrap(),
            "--destination",
            "internal",
        ],
    ));
    let copy = root.path().join("copy.db");
    good(cli(root.path(), &copy, &["unpack", pack.to_str().unwrap()]));
    let reproduced = root.path().join("reproduced");
    good(cli(
        root.path(),
        &copy,
        &[
            "render",
            "urn:example:spec",
            "--dest",
            reproduced.to_str().unwrap(),
        ],
    ));
    for path in ["example/SKILL.md", "example/scripts/run.sh"] {
        assert_eq!(
            std::fs::read(dest.join(path)).unwrap(),
            std::fs::read(reproduced.join(path)).unwrap()
        );
    }
}

#[cfg(feature = "remote")]
#[test]
fn remote_cli_requires_the_requested_historical_stamp_before_publishing() {
    use std::io::{BufRead, Read, Write};
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    std::fs::create_dir(&home).unwrap();
    let requested = "2026-01-15T00:00:00Z";
    let bytes = b"# fixture\n";
    let hash = format!(
        "sha256:{}",
        hex::encode(ring::digest::digest(&ring::digest::SHA256, bytes).as_ref())
    );
    for (returned_at, expected_success) in [(Some(requested), true), (None, false)] {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let response=serde_json::json!({"schema":1,"spec":"urn:example:requested","source_tx":7,
            "valid_at":returned_at,"files":{"report.md":{"sha256":hash,"base64":STANDARD.encode(bytes),"mode":420}}}).to_string();
        let worker = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                .unwrap();
            let mut reader = std::io::BufReader::new(stream.try_clone().unwrap());
            let mut first = String::new();
            reader.read_line(&mut first).unwrap();
            assert!(first.starts_with("POST /render "));
            let mut length = None;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" {
                    break;
                }
                if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    length = Some(value.trim().parse::<usize>().unwrap());
                }
            }
            let mut body = vec![0; length.unwrap()];
            reader.read_exact(&mut body).unwrap();
            let request: serde_json::Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(request["spec"], "urn:example:requested");
            assert_eq!(request["valid_at"], requested);
            write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",response.len(),response).unwrap();
        });
        let destination = root.path().join(if expected_success {
            "matched"
        } else {
            "mismatched"
        });
        let result = Command::new(env!("CARGO_BIN_EXE_quipu"))
            .args([
                "render",
                "urn:example:requested",
                "--dest",
                destination.to_str().unwrap(),
                "--server",
                &format!("http://{address}"),
                "--valid-at",
                requested,
            ])
            .current_dir(root.path())
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .output()
            .unwrap();
        worker.join().unwrap();
        assert_eq!(
            result.status.success(),
            expected_success,
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        if expected_success {
            assert_eq!(std::fs::read(destination.join("report.md")).unwrap(), bytes);
        } else {
            assert!(
                !destination.exists(),
                "a mismatched historical stamp must not publish a destination"
            );
        }
    }
}
