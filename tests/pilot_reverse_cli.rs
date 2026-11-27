//! `quipu pilot reverse` through the binary (aegis-4qv.3): dry run and every
//! refusal leave both files byte-identical; an execute moves every accepted
//! owner write back to legacy, and a re-run prints the identical receipt.
#![cfg(all(unix, feature = "shacl", feature = "ownership-witness-s3"))]

use quipu::Store;
use serde_json::{Value, json};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Command, Output};

const GRAPH: &str = "https://example.test/graphs/ingest";
const TAG: &str = "snapshot:code:repo";
const TS: &str = "2026-11-11T00:00:00Z";

struct Lab {
    dir: tempfile::TempDir,
}

impl Lab {
    /// enroll -> cutover -> two accepted owner writes, on disposable files.
    fn new() -> Self {
        let lab = Self {
            dir: tempfile::tempdir().unwrap(),
        };
        let mut legacy = Store::open(&lab.s("legacy.db")).unwrap();
        quipu::tool_knot(
            &mut legacy,
            &json!({"turtle": "<urn:r:f> <urn:r:p> \"a\" . <urn:r:f> <urn:r:q> <urn:r:o> .",
                    "replace_snapshot": true, "snapshot": "code:repo", "source": "lab"}),
        )
        .unwrap();
        let slice = legacy
            .export_routed_slice(TAG, quipu::schema::ROOT_GRAPH_IRI)
            .unwrap();
        let mut owner = Store::open(&lab.s("owner.db")).unwrap();
        quipu::tool_graph_create(&owner, &json!({"graph": GRAPH, "timestamp": TS})).unwrap();
        owner.import_routed_slice(&slice, TAG, GRAPH, TS).unwrap();
        legacy.fence_routed_graph(GRAPH, 7, 4).unwrap();
        legacy.cut_over_routed_source(TAG, GRAPH, 7, 4, TS).unwrap();
        for i in 0..2 {
            quipu::tool_knot(
                &mut owner,
                &json!({"turtle": format!("<urn:r:w{i}> <urn:r:n> \"{i}\" ."), "graph": GRAPH}),
            )
            .unwrap();
        }
        drop((legacy, owner));
        lab.catalog("cat7.json", 7, true);
        lab.catalog("cat8.json", 8, false);
        lab.catalog("cat8-routed.json", 8, true);
        lab
    }

    fn catalog(&self, name: &str, revision: u64, routed: bool) {
        let routes = if routed {
            json!([{"graph": GRAPH, "file_id": "ingest", "store_id": "owner-store",
                    "log_epoch": "epoch-1", "generation": 4, "policy": "policy-v1"}])
        } else {
            json!([])
        };
        let doc = json!({"version": 1, "revision": revision, "legacy_store_id": "legacy-store",
            "budget": {"max_owners": 1, "queued_per_owner": 1, "readers_per_owner": 1,
                       "max_queued": 1, "max_connections": 3},
            "routes": routes,
            "legacy_participant": {"graph": quipu::schema::ROOT_GRAPH_IRI, "file_id": "legacy",
                "store_id": "legacy-store", "log_epoch": "epoch-0", "generation": 1,
                "policy": "legacy-policy"}});
        let path = self.path(name);
        std::fs::write(&path, doc.to_string()).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    fn s(&self, name: &str) -> String {
        self.path(name).to_string_lossy().into_owned()
    }

    fn pilot(&self, extra: &[&str]) -> Output {
        let mut args = vec![
            "pilot",
            "reverse",
            "--db",
            "legacy.db",
            "--owner-file",
            "owner.db",
            "--graph",
            GRAPH,
            "--revision",
            "7",
            "--generation",
            "4",
            "--catalog",
            "cat7.json",
        ];
        args.extend_from_slice(extra);
        Command::new(env!("CARGO_BIN_EXE_quipu"))
            .args(args)
            .current_dir(self.dir.path())
            .env_remove("QUIPU_SERVER")
            .output()
            .unwrap()
    }

    fn ok(&self, extra: &[&str]) -> Value {
        let out = self.pilot(extra);
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        serde_json::from_slice(&out.stdout).unwrap()
    }

    fn bytes(&self) -> (Vec<u8>, Vec<u8>) {
        (
            std::fs::read(self.path("legacy.db")).unwrap(),
            std::fs::read(self.path("owner.db")).unwrap(),
        )
    }
}

#[test]
fn dry_run_and_refusals_leave_both_files_byte_identical() {
    let lab = Lab::new();
    let before = lab.bytes();
    let dry = lab.ok(&["--next-catalog", "cat8.json"]);
    assert_eq!(dry["mode"], "dry-run");
    assert_eq!(dry["reverse"]["state"], "planned");
    assert_eq!(dry["reverse"]["owner_facts"], 4);
    assert_eq!(dry["before"], dry["after"]);
    for (extra, why) in [
        (vec!["--next-catalog", "cat8-routed.json"], "back to legacy"),
        (vec!["--next-catalog", "cat7.json"], "R and R+1"),
        (
            vec!["--next-catalog", "cat8.json", "--token", "x"],
            "never accepted",
        ),
        (
            vec!["--next-catalog", "cat8.json", "--bogus"],
            "unknown argument",
        ),
    ] {
        let mut extra = extra;
        extra.push("--execute");
        let out = lab.pilot(&extra);
        assert!(!out.status.success());
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(stderr.contains(why), "{why}: {stderr}");
    }
    assert_eq!(lab.bytes(), before, "every refusal wrote nothing");
}

#[test]
fn execute_reverses_every_accepted_write_and_reruns_identically() {
    let lab = Lab::new();
    let owner_before = lab.bytes().1;
    let done = lab.ok(&["--next-catalog", "cat8.json", "--execute"]);
    assert_eq!(done["mode"], "execute");
    let reverse = &done["reverse"];
    assert_eq!(reverse["state"], "lifted");
    assert_eq!(reverse["to"]["generation"], 5);
    assert_eq!(reverse["to"]["catalog_revision"], 8);
    assert_eq!(reverse["witness_state"], "active_unretired");
    assert_eq!(reverse["sources"][TAG], 2);
    assert_eq!(lab.bytes().1, owner_before, "the owner file is only read");
    let legacy = Store::open_read_only(&lab.s("legacy.db")).unwrap();
    assert_eq!(legacy.export_routed_graph(GRAPH).unwrap().len(), 4);
    assert!(legacy.routed_graphs_fenced().unwrap().is_empty());
    drop(legacy);
    let again = lab.ok(&["--next-catalog", "cat8.json", "--execute"]);
    assert_eq!(
        again["reverse"], done["reverse"],
        "resume prints the same receipt"
    );
}
