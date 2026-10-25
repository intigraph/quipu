//! `quipu attest` must be operable end to end by an operator: register, read
//! the write scope back, revoke (aegis-bys8d1). The first production probe
//! found no revoke verb, so revoking took raw SQL on the live store, and `list`
//! hid the write grant the probe existed to test.
#![cfg(feature = "shacl")]

use std::process::{Command, Output};

const KEY: &str = "9935136f7cc9b267ac84aba8fad08e1fcaf3fef5c841b64515bccd1800ed4881";

fn attest(db: &std::path::Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_quipu"))
        .arg("attest")
        .args(args)
        .arg("--db")
        .arg(db)
        .output()
        .unwrap()
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn register_list_and_revoke_round_trip_through_the_cli() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("attest.db");
    let reg = attest(
        &db,
        &[
            "register",
            "--agent",
            "urn:probe:writer",
            "--session",
            "s-write",
            "--public-key",
            KEY,
            "--introducer",
            "operator",
            "--issued-at",
            "1790815248",
            "--expires-at",
            "1790901648",
            "--allow-write",
        ],
    );
    assert!(reg.status.success(), "register: {reg:?}");

    // The write grant and the expiry are visible to the operator.
    let listed = stdout(&attest(&db, &["list"]));
    assert!(listed.contains("s-write"), "{listed}");
    assert!(listed.contains("allow_write=true"), "{listed}");
    assert!(listed.contains("expires_at=1790901648"), "{listed}");
    assert!(listed.contains("revoked=false"), "{listed}");

    let rev = attest(&db, &["revoke", "s-write"]);
    assert!(rev.status.success(), "revoke: {rev:?}");
    assert!(stdout(&rev).contains("REVOKED"));
    assert!(stdout(&attest(&db, &["list"])).contains("revoked=true"));

    // An unknown session is an error, not a silent success.
    let missing = attest(&db, &["revoke", "s-absent"]);
    assert_eq!(missing.status.code(), Some(1), "{missing:?}");

    // No session named: usage, exit 2.
    let usage = attest(&db, &["revoke"]);
    assert_eq!(usage.status.code(), Some(2), "{usage:?}");
}

/// aegis-2idcev: `require-v2` and `allow-v1` are per-session operator verbs, and
/// `list` shows the result, so moving one signer to write-v2 needs no raw SQL.
#[test]
fn require_v2_is_set_and_cleared_per_session_through_the_cli() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("attest.db");
    let reg = attest(
        &db,
        &[
            "register",
            "--agent",
            "urn:probe:writer",
            "--session",
            "s-v2",
            "--public-key",
            KEY,
            "--introducer",
            "operator",
            "--issued-at",
            "1790815248",
            "--expires-at",
            "1790901648",
            "--allow-write",
        ],
    );
    assert!(reg.status.success(), "register: {reg:?}");
    assert!(stdout(&attest(&db, &["list"])).contains("require_v2=false"));

    let set = attest(&db, &["require-v2", "s-v2"]);
    assert!(set.status.success(), "require-v2: {set:?}");
    assert!(stdout(&set).contains("REQUIRED"), "{set:?}");
    assert!(stdout(&attest(&db, &["list"])).contains("require_v2=true"));

    let clear = attest(&db, &["allow-v1", "s-v2"]);
    assert!(clear.status.success(), "allow-v1: {clear:?}");
    assert!(stdout(&attest(&db, &["list"])).contains("require_v2=false"));

    let missing = attest(&db, &["require-v2", "s-absent"]);
    assert_eq!(missing.status.code(), Some(1), "{missing:?}");
    let usage = attest(&db, &["require-v2"]);
    assert_eq!(usage.status.code(), Some(2), "{usage:?}");
}
