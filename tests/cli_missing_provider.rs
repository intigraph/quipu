//! An automatic-embedding write must not silently succeed without a provider.
#![cfg(feature = "shacl")]

use std::process::{Command, Output};

fn fixture(auto_embed: bool) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join(".bobbin")).unwrap();
    std::fs::write(
        dir.path().join(".bobbin/config.toml"),
        format!("[quipu.embedding]\nauto_embed = {auto_embed}\n"),
    )
    .unwrap();
    std::fs::write(dir.path().join("input.ttl"), "<urn:a> <urn:p> <urn:b> .\n").unwrap();
    dir
}

fn run(dir: &std::path::Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_quipu"))
        .args(args)
        .current_dir(dir)
        .env("HOME", dir)
        .env("XDG_CONFIG_HOME", dir.join("config"))
        .env("XDG_DATA_HOME", dir.join("data"))
        .env("XDG_CACHE_HOME", dir.join("cache"))
        .env_remove("QUIPU_CONFIG")
        .env_remove("ORT_DYLIB_PATH")
        .output()
        .unwrap()
}

#[test]
fn automatic_embedding_without_provider_refuses_once_before_writing() {
    let dir = fixture(true);
    let out = run(dir.path(), &["knot", "input.ttl", "--db", "graph.db"]);
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    assert_eq!(
        String::from_utf8_lossy(&out.stderr),
        format!("{}\n", quipu::embedding::NO_PROVIDER_HELP)
    );
    assert!(out.stdout.is_empty(), "{out:?}");
    let read = run(
        dir.path(),
        &[
            "read",
            "SELECT ?s WHERE { ?s <urn:p> ?o }",
            "--db",
            "graph.db",
        ],
    );
    assert!(read.status.success(), "{read:?}");
    assert!(read.stderr.is_empty(), "{read:?}");
    assert!(!String::from_utf8_lossy(&read.stdout).contains("urn:a"));
}

#[test]
fn explicit_graph_only_opt_out_still_allows_writes() {
    let dir = fixture(false);
    let out = run(dir.path(), &["knot", "input.ttl", "--db", "graph.db"]);
    assert!(out.status.success(), "{out:?}");
    assert!(String::from_utf8_lossy(&out.stdout).contains("knotted 1 facts"));
    assert!(!String::from_utf8_lossy(&out.stderr).contains("no embedding provider configured"));
}
