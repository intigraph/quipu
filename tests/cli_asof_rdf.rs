#![cfg(all(unix, feature = "shacl"))]
//! Actual offline CLI: private paths, no ambient database, finite limits.

use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::{Value, json};

struct Fixture {
    root: tempfile::TempDir,
    binding: PathBuf,
    frames: PathBuf,
    scratch: PathBuf,
}

fn private(path: &Path, bytes: &[u8]) {
    fs::write(path, bytes).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
}

fn fixture() -> Fixture {
    let root = tempfile::tempdir().unwrap();
    let binding = root.path().join("binding.json");
    let frames = root.path().join("frames.jsonl");
    let scratch = root.path().join("scratch");
    fs::create_dir(&scratch).unwrap();
    fs::set_permissions(&scratch, fs::Permissions::from_mode(0o700)).unwrap();
    let record = json!({"entity":"urn:item:1","attribute":"urn:predicate:label",
        "value":"offline","graph":"urn:board","op":"assert","tx":1,
        "timestamp":"2026-11-04T00:00:00Z","valid_from":"2026-11-04T00:00:00Z","valid_to":null});
    let manifest = json!({
        "schema":"quipu-asof-share/v1","source":{"store_id":"test","log_epoch":"epoch"},
        "watermark_tx":3,"source_head_tx":3,"scope":["urn:board"],"context_graphs":[],
        "scope_digest":"sha256:scope","policy_digest":"sha256:policy","shapes_hash":"sha256:profile",
        "completeness":{"valid_time":true,"bytes":"lossless","graphs":{
            "urn:board":{"records":1,"digest":quipu::applier::share_graph_digest(&[&record]),"frozen":null}}},
        "producer":{"version":"test","git_sha":"test","git_dirty":false}});
    private(&frames,json!({"event":"quipu.asof","data":json!({"manifest":manifest,"records":[record]}).to_string()}).to_string().as_bytes());
    private(
        &binding,
        json!({"store_id":"test","log_epoch":"epoch","watermark_tx":3,
        "graph":"urn:board","shapes_hash":"sha256:profile","max_input_bytes":100000,
        "max_frames":20,"max_disk_bytes":67108864,"wall_seconds":30})
        .to_string()
        .as_bytes(),
    );
    Fixture {
        root,
        binding,
        frames,
        scratch,
    }
}

fn command(f: &Fixture) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_quipu"));
    command
        .current_dir(f.root.path())
        .env("HOME", f.root.path())
        .env("XDG_CONFIG_HOME", f.root.path().join("config"))
        .env_remove("QUIPU_CONFIG")
        .env("SQLITE_TMPDIR", &f.scratch)
        .args(["asof-rdf", "--binding"])
        .arg(&f.binding)
        .arg("--frames")
        .arg(&f.frames)
        .arg("--scratch")
        .arg(&f.scratch);
    command
}

fn succeeded(output: &Output) {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn finite_conversion_writes_a_complete_private_cut_and_refuses_overwrite() {
    let f = fixture();
    succeeded(&command(&f).output().unwrap());
    let receipt = fs::read(f.scratch.join("complete.json")).unwrap();
    let parsed: Value = serde_json::from_slice(&receipt).unwrap();
    assert_eq!(parsed["receipt"]["source_tx"], 3);
    assert_eq!(parsed["receipt"]["chunks"], 1);
    assert!(
        fs::read_to_string(f.scratch.join("chunk-00000000.ttl"))
            .unwrap()
            .contains("offline")
    );
    assert!(!f.root.path().join("quipu.db").exists());
    assert!(!command(&f).output().unwrap().status.success());
    assert_eq!(receipt, fs::read(f.scratch.join("complete.json")).unwrap());
    for entry in fs::read_dir(&f.scratch).unwrap() {
        assert_eq!(
            entry.unwrap().metadata().unwrap().permissions().mode() & 0o077,
            0
        );
    }
}

#[test]
fn unsafe_paths_and_ambient_database_choices_refuse_before_creation() {
    for case in ["tempdir", "symlink", "permissions", "db"] {
        let f = fixture();
        let mut cmd = command(&f);
        match case {
            "tempdir" => {
                cmd.env("SQLITE_TMPDIR", f.root.path());
            }
            "symlink" => {
                let link = f.root.path().join("link");
                symlink(&f.frames, &link).unwrap();
                // Replace only the invocation input, never the fixture itself.
                cmd = Command::new(env!("CARGO_BIN_EXE_quipu"));
                cmd.current_dir(f.root.path())
                    .env("SQLITE_TMPDIR", &f.scratch)
                    .args(["asof-rdf", "--binding"])
                    .arg(&f.binding)
                    .arg("--frames")
                    .arg(link)
                    .arg("--scratch")
                    .arg(&f.scratch);
            }
            "permissions" => {
                fs::set_permissions(&f.frames, fs::Permissions::from_mode(0o644)).unwrap();
            }
            _ => {
                cmd.arg("--db").arg(f.root.path().join("foreign.db"));
            }
        }
        assert!(!cmd.output().unwrap().status.success(), "{case}");
        assert!(fs::read_dir(&f.scratch).unwrap().next().is_none(), "{case}");
        assert!(!f.root.path().join("foreign.db").exists());
    }
}

#[test]
fn incomplete_or_over_budget_input_never_writes_completion() {
    for case in ["incomplete", "bytes", "disk"] {
        let f = fixture();
        if case == "incomplete" {
            private(
                &f.frames,
                b"{\"event\":\"quipu.asof.begin\",\"data\":\"{}\"}\n",
            );
        } else {
            let mut binding: Value =
                serde_json::from_slice(&fs::read(&f.binding).unwrap()).unwrap();
            binding[if case == "bytes" {
                "max_input_bytes"
            } else {
                "max_disk_bytes"
            }] = json!(1);
            private(&f.binding, &serde_json::to_vec(&binding).unwrap());
        }
        assert!(!command(&f).output().unwrap().status.success(), "{case}");
        assert!(!f.scratch.join("complete.json").exists(), "{case}");
    }
}
