//! Without `ownership-witness-s3` the CLI has no `pilot` surface at all: the
//! help is unchanged and the verb is not dispatched (aegis-4qv.1).
#![cfg(all(feature = "shacl", not(feature = "ownership-witness-s3")))]

use std::process::Command;

#[test]
fn default_build_help_has_no_pilot_and_the_verb_is_not_dispatched() {
    let dir = tempfile::tempdir().unwrap();
    let help = Command::new(env!("CARGO_BIN_EXE_quipu"))
        .arg("--help")
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(!String::from_utf8_lossy(&help.stdout).contains("pilot"));
    let out = Command::new(env!("CARGO_BIN_EXE_quipu"))
        .args(["pilot", "catalog", "--validate", "absent.json"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(!String::from_utf8_lossy(&out.stdout).contains("\"verb\""));
}
