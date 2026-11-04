//! A first-run local store is the README's quick start. It has no embedding
//! model and that is normal, so `knot` and `read` must leave stderr empty:
//! the README demo printed a missing-model notice and a JSON search record
//! between the user's commands (aegis-4pbw).
#![cfg(feature = "shacl")]

use std::process::{Command, Output};

fn quipu(dir: &std::path::Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_quipu"))
        .args(args)
        .current_dir(dir)
        .env("HOME", dir)
        .env("XDG_CONFIG_HOME", dir.join("config"))
        .env_remove("QUIPU_CONFIG")
        .output()
        .unwrap()
}

#[test]
fn first_run_knot_and_read_print_nothing_on_stderr() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("ada.ttl"),
        "@prefix : <urn:demo:> .\n:Ada :studies :Engine .\n",
    )
    .unwrap();

    let knot = quipu(dir.path(), &["knot", "ada.ttl", "--db", "demo.db"]);
    assert!(knot.status.success(), "{knot:?}");
    assert_eq!(String::from_utf8_lossy(&knot.stderr), "");
    assert!(String::from_utf8_lossy(&knot.stdout).contains("knotted 1 facts"));

    let read = quipu(
        dir.path(),
        &["read", "SELECT ?s WHERE { ?s ?p ?o }", "--db", "demo.db"],
    );
    assert!(read.status.success(), "{read:?}");
    assert_eq!(String::from_utf8_lossy(&read.stderr), "");
    assert!(String::from_utf8_lossy(&read.stdout).contains("urn:demo:Ada"));
}
