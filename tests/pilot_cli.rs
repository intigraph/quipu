//! `quipu pilot …` exercised through the binary (aegis-4qv.1). Every refusal
//! must leave its target file byte-identical; the lab sequence must land the
//! producer's slice in the owner with identical query bindings.
#![cfg(all(unix, feature = "shacl", feature = "ownership-witness-s3"))]

#[path = "support/pilot_lab.rs"]
mod lab;
use lab::{GRAPH, Lab, TAG, TS, fence_args};
use serde_json::{Value, json};
use std::os::unix::fs::PermissionsExt;

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
