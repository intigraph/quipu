//! `quipu pilot …` exercised through the binary (aegis-4qv.1). Every refusal
//! must leave its target file byte-identical; the lab sequence must land the
//! producer's slice in the owner with identical query bindings.
#![cfg(all(unix, feature = "shacl", feature = "ownership-witness-s3"))]

use quipu::Store;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const GRAPH: &str = "https://example.test/graphs/ingest";
const TAG: &str = "snapshot:code:repo";
const TS: &str = "2026-11-11T00:00:00Z";
const CODE: &str = "<urn:slice:file> <urn:slice:defines> <urn:slice:symbol> . \
    <urn:slice:file> <urn:slice:path> \"src/lib.rs\" . \
    <urn:slice:file> <urn:slice:size> 42 . \
    <urn:slice:file> <urn:slice:changed> \"2026-11-01\"^^<http://www.w3.org/2001/XMLSchema#date> . \
    <urn:slice:file> <urn:slice:note> \"hello\"@en .";

struct Lab {
    dir: tempfile::TempDir,
}

impl Lab {
    fn new() -> Self {
        let lab = Self {
            dir: tempfile::tempdir().unwrap(),
        };
        let mut legacy = Store::open(lab.path("legacy.db").to_str().unwrap()).unwrap();
        quipu::tool_knot(
            &mut legacy,
            &json!({"turtle": CODE, "replace_snapshot": true, "snapshot": "code:repo", "source": "lab"}),
        )
        .unwrap();
        drop(legacy);
        let owner = Store::open(lab.path("owner.db").to_str().unwrap()).unwrap();
        quipu::tool_graph_create(&owner, &json!({"graph": GRAPH, "timestamp": TS})).unwrap();
        drop(owner);
        lab
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    fn s(&self, name: &str) -> String {
        self.path(name).to_string_lossy().into_owned()
    }

    fn pilot(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_quipu"))
            .arg("pilot")
            .args(args)
            .current_dir(self.dir.path())
            .env_remove("QUIPU_SERVER")
            .output()
            .unwrap()
    }

    fn ok(&self, args: &[&str]) -> Value {
        let out = self.pilot(args);
        assert!(
            out.status.success(),
            "pilot {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        serde_json::from_slice(&out.stdout).unwrap()
    }

    /// Run a sabotaged verb: it must refuse and leave `target` byte-identical.
    fn refuses(&self, target: &str, args: &[&str], why: &str) {
        let before = hash(&self.path(target));
        let out = self.pilot(args);
        assert!(!out.status.success(), "pilot {args:?} should refuse");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(stderr.contains(why), "refusal names `{why}`: {stderr}");
        assert_eq!(
            hash(&self.path(target)),
            before,
            "{target} changed on refusal"
        );
    }

    fn rows(&self, file: &str, graph: Option<&str>) -> Vec<String> {
        let store = Store::open_read_only(&self.s(file)).unwrap();
        let mut input = json!({"query":
            "SELECT ?s ?p ?o WHERE { ?s ?p ?o FILTER(STRSTARTS(STR(?s), \"urn:slice:\")) }"});
        if let Some(graph) = graph {
            input["graph"] = json!(graph);
        }
        let out = quipu::tool_query(&store, &input).unwrap();
        let mut rows: Vec<String> = out["rows"]
            .as_array()
            .unwrap()
            .iter()
            .map(ToString::to_string)
            .collect();
        rows.sort();
        rows
    }
}

/// Main file plus WAL sidecar, so an uncheckpointed write still counts.
fn hash(path: &Path) -> String {
    let mut h = Sha256::new();
    for p in [
        path.to_path_buf(),
        PathBuf::from(format!("{}-wal", path.display())),
    ] {
        if let Ok(bytes) = std::fs::read(&p) {
            h.update(&bytes);
        }
        h.update(b"|");
    }
    hex::encode(h.finalize())
}

fn fence_args<'a>(db: &'a str, generation: &'a str) -> Vec<&'a str> {
    vec![
        "fence",
        "--db",
        db,
        "--graph",
        GRAPH,
        "--revision",
        "7",
        "--generation",
        generation,
    ]
}

#[test]
fn lab_sequence_export_import_fence_cutover_keeps_bindings() {
    let lab = Lab::new();
    let (legacy, owner) = (lab.s("legacy.db"), lab.s("owner.db"));
    let root_rows = lab.rows("legacy.db", None);
    assert_eq!(root_rows.len(), 5);

    // export-slice: dry run writes nothing, then execute writes a 0600 file.
    let dry = lab.ok(&[
        "export-slice",
        "--db",
        &legacy,
        "--tag",
        TAG,
        "--graph",
        quipu::schema::ROOT_GRAPH_IRI,
        "--out",
        "slice.json",
    ]);
    assert_eq!(dry["mode"], "dry-run");
    assert!(!lab.path("slice.json").exists());
    let exported = lab.ok(&[
        "export-slice",
        "--db",
        &legacy,
        "--tag",
        TAG,
        "--graph",
        quipu::schema::ROOT_GRAPH_IRI,
        "--out",
        "slice.json",
        "--execute",
    ]);
    assert_eq!(exported["count"], 5);
    assert_eq!(exported["before"]["sha256"], exported["after"]["sha256"]);
    let mode = std::fs::metadata(lab.path("slice.json"))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);
    // The slice is the producer's ROOT facts; import targets the routed graph.
    let mut doc: Value =
        serde_json::from_slice(&std::fs::read(lab.path("slice.json")).unwrap()).unwrap();
    doc["graph"] = json!(GRAPH);
    std::fs::write(lab.path("routed.json"), doc.to_string()).unwrap();

    let dry = lab.ok(&[
        "import-slice",
        "--owner-file",
        &owner,
        "--in",
        "routed.json",
    ]);
    assert_eq!(dry["before"], dry["after"]);
    let imported = lab.ok(&[
        "import-slice",
        "--owner-file",
        &owner,
        "--in",
        "routed.json",
        "--at",
        TS,
        "--execute",
    ]);
    assert_eq!(
        imported["landed_multiset_sha256"],
        exported["multiset_sha256"]
    );
    assert_ne!(imported["before"]["sha256"], imported["after"]["sha256"]);

    lab.ok(&[&fence_args(&legacy, "4")[..], &["--execute"]].concat());
    let verified = lab.ok(&["fence", "--verify", "--db", &legacy]);
    assert_eq!(verified["verified"], true);
    let cut = lab.ok(&[
        "cutover",
        "--db",
        &legacy,
        "--tag",
        TAG,
        "--graph",
        GRAPH,
        "--revision",
        "7",
        "--generation",
        "4",
        "--at",
        "2026-11-12T00:00:00Z",
        "--execute",
    ]);
    assert_eq!(cut["retracted"], 5);

    // Parity: the owner's graph answers exactly what ROOT answered before.
    assert_eq!(lab.rows("owner.db", Some(GRAPH)), root_rows);
    assert!(
        lab.rows("legacy.db", None).is_empty(),
        "ROOT no longer serves the moved slice"
    );
}

#[test]
fn each_guard_refuses_and_leaves_the_target_byte_identical() {
    let lab = Lab::new();
    let (legacy, owner) = (lab.s("legacy.db"), lab.s("owner.db"));
    let cut = |generation: &'static str| {
        vec![
            "cutover",
            "--db",
            legacy.as_str(),
            "--tag",
            TAG,
            "--graph",
            GRAPH,
            "--revision",
            "7",
            "--generation",
            generation,
            "--execute",
        ]
    };
    // Unfenced cutover.
    lab.refuses("legacy.db", &cut("4"), "not fenced");
    // Credentials on argv refuse before any file is opened.
    lab.refuses(
        "legacy.db",
        &[&fence_args(&legacy, "4")[..], &["--secret-access-key", "x"]].concat(),
        "never accepted on argv",
    );
    // Stale generation against the installed fence.
    lab.ok(&[&fence_args(&legacy, "4")[..], &["--execute"]].concat());
    lab.refuses("legacy.db", &cut("3"), "differ");
    lab.refuses(
        "legacy.db",
        &[&fence_args(&legacy, "3")[..], &["--execute"]].concat(),
        "another revision or generation",
    );

    lab.ok(&[
        "export-slice",
        "--db",
        &legacy,
        "--tag",
        TAG,
        "--graph",
        quipu::schema::ROOT_GRAPH_IRI,
        "--out",
        "slice.json",
        "--execute",
    ]);
    let mut doc: Value =
        serde_json::from_slice(&std::fs::read(lab.path("slice.json")).unwrap()).unwrap();
    // Graph mismatch: the owner never registered this graph.
    doc["graph"] = json!("https://example.test/graphs/other");
    std::fs::write(lab.path("other.json"), doc.to_string()).unwrap();
    lab.refuses(
        "owner.db",
        &[
            "import-slice",
            "--owner-file",
            &owner,
            "--in",
            "other.json",
            "--execute",
        ],
        "not registered",
    );
    // A tampered slice refuses on its digest.
    doc["graph"] = json!(GRAPH);
    doc["facts"][0]["valid_from"] = json!("1999-01-01T00:00:00Z");
    std::fs::write(lab.path("tampered.json"), doc.to_string()).unwrap();
    lab.refuses(
        "owner.db",
        &[
            "import-slice",
            "--owner-file",
            &owner,
            "--in",
            "tampered.json",
            "--execute",
        ],
        "multiset_sha256",
    );
    // A second import refuses.
    let mut doc: Value =
        serde_json::from_slice(&std::fs::read(lab.path("slice.json")).unwrap()).unwrap();
    doc["graph"] = json!(GRAPH);
    std::fs::write(lab.path("routed.json"), doc.to_string()).unwrap();
    lab.ok(&[
        "import-slice",
        "--owner-file",
        &owner,
        "--in",
        "routed.json",
        "--execute",
    ]);
    lab.refuses(
        "owner.db",
        &[
            "import-slice",
            "--owner-file",
            &owner,
            "--in",
            "routed.json",
            "--execute",
        ],
        "imports once",
    );
}

fn catalog_doc() -> Value {
    json!({"version": 1, "revision": 7, "legacy_store_id": "legacy-store",
        "budget": {"max_owners": 1, "queued_per_owner": 1, "readers_per_owner": 1,
                   "max_queued": 1, "max_connections": 3},
        "routes": [{"graph": GRAPH, "file_id": "ingest", "store_id": "owner-store",
                    "log_epoch": "epoch-1", "generation": 4, "policy": "policy-v1"}],
        "legacy_participant": {"graph": quipu::schema::ROOT_GRAPH_IRI, "file_id": "legacy",
            "store_id": "legacy-store", "log_epoch": "epoch-0", "generation": 1,
            "policy": "legacy-policy"}})
}

#[test]
fn catalog_write_validates_and_refuses_group_writable() {
    let lab = Lab::new();
    std::fs::write(lab.path("input.json"), catalog_doc().to_string()).unwrap();
    let dry = lab.ok(&["catalog", "--write", "catalog.json", "--from", "input.json"]);
    assert_eq!(dry["mode"], "dry-run");
    assert!(!lab.path("catalog.json").exists());
    let written = lab.ok(&[
        "catalog",
        "--write",
        "catalog.json",
        "--from",
        "input.json",
        "--execute",
    ]);
    assert_eq!(written["catalog"]["revision"], 7);
    let mode = std::fs::metadata(lab.path("catalog.json"))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);
    assert_eq!(
        lab.ok(&["catalog", "--validate", "catalog.json"])["valid"],
        true
    );
    // Never overwrite a published catalog.
    lab.refuses(
        "catalog.json",
        &[
            "catalog",
            "--write",
            "catalog.json",
            "--from",
            "input.json",
            "--execute",
        ],
        "already exists",
    );
    // Group-writable refuses validation, unchanged.
    std::fs::set_permissions(
        lab.path("catalog.json"),
        std::fs::Permissions::from_mode(0o620),
    )
    .unwrap();
    lab.refuses(
        "catalog.json",
        &["catalog", "--validate", "catalog.json"],
        "refused",
    );
    // A malformed input refuses before any file is created.
    let mut bad = catalog_doc();
    bad["version"] = json!(2);
    std::fs::write(lab.path("bad.json"), bad.to_string()).unwrap();
    let out = lab.pilot(&[
        "catalog",
        "--write",
        "new.json",
        "--from",
        "bad.json",
        "--execute",
    ]);
    assert!(!out.status.success());
    assert!(!lab.path("new.json").exists());
}

#[test]
fn an_unknown_verb_refuses_loudly() {
    let lab = Lab::new();
    // Every pilot verb is implemented (enroll: aegis-4qv.2, reverse: aegis-4qv.3).
    // An unknown verb still refuses loudly, with the usage text and exit 2.
    let out = lab.pilot(&["frobnicate"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("usage: quipu pilot"));
}
