//! `[quipu.shacl]` must reach a CLI write, not only the server.
//!
//! `server.rs` copies `config.shacl` into its store; `cli_open.rs` did not, so
//! the `quipu` binary ran on the store's built-in default whatever the config
//! file said. Once that default became ON, an operator who opted out with
//! `validate_on_write = false` still had every non-conformant `quipu knot`
//! refused -- the opt-out the config reference documents did not work from the
//! CLI.
//!
//! Every case writes the SAME non-conformant payload against the SAME loaded
//! shape, so the only variable is the config file. The default cases (absent
//! file, empty section) must REFUSE, which proves the shape is enforced at all;
//! without them an explicit-false acceptance could just mean nothing is
//! validated.

// The `quipu` binary is `required-features = ["shacl"]` (see
// search_config_cli.rs for why this gate is load-bearing).
#![cfg(feature = "shacl")]

use std::path::Path;
use std::process::{Command, Output};

const SHAPE: &str = "\
@prefix sh: <http://www.w3.org/ns/shacl#> .
@prefix ex: <http://example.test/> .
ex:ThingShape a sh:NodeShape ;
    sh:targetClass ex:Thing ;
    sh:property [ sh:path ex:name ; sh:maxCount 1 ] .
";

/// An `ex:Thing` with two `ex:name`s where the loaded shape allows one.
///
/// maxCount, not minCount: a plain `knot` is judged by the post-state check
/// (`store/shacl_poststate.rs`), which enforces cardinality ceilings and value
/// constraints; required-value floors are the episode lane's.
const NON_CONFORMANT: &str = "\
@prefix ex: <http://example.test/> .
ex:bad a ex:Thing ; ex:name \"one\", \"two\" .
";

const CONFORMANT: &str = "\
@prefix ex: <http://example.test/> .
ex:good a ex:Thing ; ex:name \"good\" .
";

/// A fresh store in `dir` with `SHAPE` loaded, and the payload files beside it.
fn store_with_loaded_shape(dir: &Path) -> String {
    let db = dir.join("store.db");
    let path = db.to_str().unwrap().to_string();
    let mut store = quipu::Store::open(&path).unwrap();
    store
        .load_shapes("cli-config-test", SHAPE, "2026-10-21T00:00:00Z")
        .unwrap();
    std::fs::write(dir.join("bad.ttl"), NON_CONFORMANT).unwrap();
    std::fs::write(dir.join("good.ttl"), CONFORMANT).unwrap();
    path
}

fn write_config(dir: &Path, body: &str) {
    std::fs::create_dir_all(dir.join(".bobbin")).unwrap();
    std::fs::write(dir.join(".bobbin/config.toml"), body).unwrap();
}

fn knot(dir: &Path, db: &str, file: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_quipu"))
        .current_dir(dir)
        .args(["knot", file, "--db", db])
        .output()
        .unwrap()
}

fn subject_stored(dir: &Path, db: &str, iri: &str) -> bool {
    let out = Command::new(env!("CARGO_BIN_EXE_quipu"))
        .current_dir(dir)
        .args(["query", &format!("ASK {{ <{iri}> ?p ?o }}"), "--db", db])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).contains("true")
}

fn describe(out: &Output) -> String {
    format!(
        "status {:?}\nstdout: {}\nstderr: {}",
        out.status.code(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

const BAD: &str = "http://example.test/bad";

#[test]
fn absent_config_refuses_a_non_conformant_cli_write() {
    let dir = tempfile::tempdir().unwrap();
    let db = store_with_loaded_shape(dir.path());

    // CONTROL: the same store and shape accept a conformant write, so a refusal
    // below is the shape firing, not the store or the command failing.
    let good = knot(dir.path(), &db, "good.ttl");
    assert!(
        good.status.success(),
        "conformant write failed: {}",
        describe(&good)
    );

    let bad = knot(dir.path(), &db, "bad.ttl");
    assert!(
        !bad.status.success(),
        "default (no config) must enforce loaded shapes: {}",
        describe(&bad)
    );
    assert!(
        !subject_stored(dir.path(), &db, BAD),
        "refused write left data behind"
    );
}

#[test]
fn empty_shacl_section_keeps_the_default_on() {
    let dir = tempfile::tempdir().unwrap();
    let db = store_with_loaded_shape(dir.path());
    write_config(dir.path(), "[quipu.shacl]\n");

    let bad = knot(dir.path(), &db, "bad.ttl");
    assert!(
        !bad.status.success(),
        "an empty [quipu.shacl] section must not switch validation off: {}",
        describe(&bad)
    );
    assert!(
        !subject_stored(dir.path(), &db, BAD),
        "refused write left data behind"
    );
}

#[test]
fn explicit_opt_out_reaches_the_cli() {
    let dir = tempfile::tempdir().unwrap();
    let db = store_with_loaded_shape(dir.path());
    write_config(dir.path(), "[quipu.shacl]\nvalidate_on_write = false\n");

    // The bug: the CLI ignored this file and refused exactly like the default.
    let bad = knot(dir.path(), &db, "bad.ttl");
    assert!(
        bad.status.success(),
        "explicit validate_on_write = false was ignored by the CLI: {}",
        describe(&bad)
    );
    assert!(
        subject_stored(dir.path(), &db, BAD),
        "accepted write was not stored"
    );
}
