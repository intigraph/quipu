//! `quipu pilot import-slice --into` through the binary (aegis-4qv.6).
#![cfg(all(unix, feature = "shacl", feature = "ownership-witness-s3"))]

#[path = "support/pilot_lab.rs"]
mod lab;
use lab::{GRAPH, Lab, TAG, TS, fence_args, hash};
use quipu::Store;
use serde_json::{Value, json};

/// aegis-4qv.6: `--into G --legacy-db LEGACY` lands a ROOT slice in the routed
/// graph G without hand-editing the slice file, and refuses any target the
/// legacy file has not fenced for that source.
#[test]
fn import_slice_into_retargets_a_root_slice_only_to_the_fenced_graph() {
    let lab = Lab::new();
    let (legacy, owner) = (lab.s("legacy.db"), lab.s("owner.db"));
    let root_rows = lab.rows("legacy.db", None);
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
    let into = |graph: &'static str, execute: bool| {
        let mut args = vec![
            "import-slice",
            "--owner-file",
            owner.as_str(),
            "--in",
            "slice.json",
            "--into",
            graph,
            "--legacy-db",
            legacy.as_str(),
            "--at",
            TS,
        ];
        if execute {
            args.push("--execute");
        }
        args
    };
    // Unfenced: G is not routed for this source yet; neither file changes.
    lab.refuses("owner.db", &into(GRAPH, true), "not fenced");
    lab.refuses("legacy.db", &into(GRAPH, true), "not fenced");
    // Half a retarget refuses.
    lab.refuses(
        "owner.db",
        &[
            "import-slice",
            "--owner-file",
            &owner,
            "--in",
            "slice.json",
            "--into",
            GRAPH,
            "--execute",
        ],
        "together",
    );

    lab.ok(&[&fence_args(&legacy, "4")[..], &["--execute"]].concat());
    // A different target than the fenced G refuses with nothing written.
    let other = "https://example.test/graphs/other";
    lab.refuses("owner.db", &into(other, true), "not fenced");
    lab.refuses(
        "owner.db",
        &into(quipu::schema::ROOT_GRAPH_IRI, true),
        "not ROOT",
    );
    // A fenced legacy file that is not this slice's source refuses.
    let foreign = lab.s("foreign.db");
    drop(Store::open(&foreign).unwrap());
    lab.ok(&[&fence_args(&foreign, "4")[..], &["--execute"]].concat());
    lab.refuses(
        "owner.db",
        &[
            "import-slice",
            "--owner-file",
            &owner,
            "--in",
            "slice.json",
            "--into",
            GRAPH,
            "--legacy-db",
            &foreign,
            "--execute",
        ],
        "does not hold this slice",
    );
    // A slice already naming a routed graph is not retargeted.
    let mut doc: Value =
        serde_json::from_slice(&std::fs::read(lab.path("slice.json")).unwrap()).unwrap();
    doc["graph"] = json!(GRAPH);
    std::fs::write(lab.path("routed.json"), doc.to_string()).unwrap();
    lab.refuses(
        "owner.db",
        &[
            "import-slice",
            "--owner-file",
            &owner,
            "--in",
            "routed.json",
            "--into",
            GRAPH,
            "--legacy-db",
            &legacy,
            "--execute",
        ],
        "from ROOT only",
    );

    // Dry run first: the plan records the retarget and writes nothing.
    let dry = lab.ok(&into(GRAPH, false));
    assert_eq!(dry["mode"], "dry-run");
    assert_eq!(dry["before"], dry["after"]);
    assert_eq!(dry["plan"]["graph"], GRAPH);
    assert_eq!(
        dry["plan"]["retarget"]["from_graph"],
        quipu::schema::ROOT_GRAPH_IRI
    );
    assert_eq!(dry["plan"]["retarget"]["into_graph"], GRAPH);

    let legacy_before = hash(&lab.path("legacy.db"));
    let imported = lab.ok(&into(GRAPH, true));
    assert_eq!(
        hash(&lab.path("legacy.db")),
        legacy_before,
        "legacy untouched"
    );
    let retarget = &imported["plan"]["retarget"];
    assert_eq!(retarget["from_graph"], quipu::schema::ROOT_GRAPH_IRI);
    assert_eq!(retarget["into_graph"], GRAPH);
    assert_eq!(retarget["legacy_fence"]["catalog_revision"], 7);
    assert_eq!(retarget["legacy_fence"]["generation"], 4);
    // The landed multiset is the slice's own.
    assert_eq!(
        imported["plan"]["multiset_sha256"],
        exported["multiset_sha256"]
    );
    assert_eq!(
        imported["landed_multiset_sha256"],
        exported["multiset_sha256"]
    );
    assert_eq!(lab.rows("owner.db", Some(GRAPH)), root_rows);
    // And it still imports once.
    lab.refuses("owner.db", &into(GRAPH, true), "imports once");
}
