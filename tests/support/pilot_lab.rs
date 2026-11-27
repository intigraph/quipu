//! The `quipu pilot` lab shared by the binary-level pilot tests: a legacy
//! file holding a code snapshot in ROOT and an owner with the routed graph.
#![allow(dead_code)]

use quipu::Store;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

pub const GRAPH: &str = "https://example.test/graphs/ingest";
pub const TAG: &str = "snapshot:code:repo";
pub const TS: &str = "2026-11-11T00:00:00Z";
pub const CODE: &str = "<urn:slice:file> <urn:slice:defines> <urn:slice:symbol> . \
    <urn:slice:file> <urn:slice:path> \"src/lib.rs\" . \
    <urn:slice:file> <urn:slice:size> 42 . \
    <urn:slice:file> <urn:slice:changed> \"2026-11-01\"^^<http://www.w3.org/2001/XMLSchema#date> . \
    <urn:slice:file> <urn:slice:note> \"hello\"@en .";

pub struct Lab {
    pub dir: tempfile::TempDir,
}

impl Lab {
    pub fn new() -> Self {
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

    pub fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    pub fn s(&self, name: &str) -> String {
        self.path(name).to_string_lossy().into_owned()
    }

    pub fn pilot(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_quipu"))
            .arg("pilot")
            .args(args)
            .current_dir(self.dir.path())
            .env_remove("QUIPU_SERVER")
            .output()
            .unwrap()
    }

    pub fn ok(&self, args: &[&str]) -> Value {
        let out = self.pilot(args);
        assert!(
            out.status.success(),
            "pilot {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        serde_json::from_slice(&out.stdout).unwrap()
    }

    /// Run a sabotaged verb: it must refuse and leave `target` byte-identical.
    pub fn refuses(&self, target: &str, args: &[&str], why: &str) {
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

    pub fn rows(&self, file: &str, graph: Option<&str>) -> Vec<String> {
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
pub fn hash(path: &Path) -> String {
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

pub fn fence_args<'a>(db: &'a str, generation: &'a str) -> Vec<&'a str> {
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
