//! The real-Git qpack fixture shared by the Git merge tests.
#![allow(dead_code)]
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tempfile::TempDir;

pub const NS: &str = "http://neuralamplifier.local/ontology/smac/";
pub fn git(p: &Path, args: &[&str]) -> Output {
    Command::new("git")
        .args([
            "-c",
            "user.name=Merge test",
            "-c",
            "user.email=test@example.org",
            "-C",
        ])
        .arg(p)
        .args(args)
        .output()
        .unwrap()
}
pub fn g(p: &Path, args: &[&str]) -> String {
    let o = git(p, args);
    assert!(
        o.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8(o.stdout).unwrap().trim().into()
}
pub fn cli(p: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_quipu"))
        .current_dir(p)
        .args(args)
        .output()
        .unwrap()
}
pub fn hash(b: &[u8]) -> String {
    format!(
        "sha256:{}",
        hex::encode(ring::digest::digest(&ring::digest::SHA256, b).as_ref())
    )
}
pub fn rehash(p: &Path) {
    let graph =
        quipu::share::canonicalize_ntriples(&std::fs::read(p.join("qpack/export.nt")).unwrap())
            .unwrap();
    std::fs::write(p.join("qpack/export.nt"), &graph).unwrap();
    let file = p.join("qpack/manifest.json");
    let mut m: Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
    m["graph_hash"] = hash(&graph).into();
    m["shapes_hash"] = hash(&std::fs::read(p.join("qpack/shapes.ttl")).unwrap()).into();
    m.as_object_mut().unwrap().remove("share_id");
    m.as_object_mut().unwrap().remove("attestation");
    m["share_id"] = hash(&serde_json::to_vec(&m).unwrap()).into();
    std::fs::write(file, serde_json::to_vec(&m).unwrap()).unwrap();
}
pub struct Fixture {
    pub temp: TempDir,
    pub base: String,
    pub ours: String,
    pub theirs: String,
}
impl Fixture {
    pub fn path(&self) -> &Path {
        self.temp.path()
    }
    pub fn new(kind: &str) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let p = temp.path();
        g(p, &["init", "-b", "base"]);
        g(p, &["config", "user.name", "Merge test"]);
        g(p, &["config", "user.email", "test@example.org"]);
        std::fs::create_dir(p.join("qpack")).unwrap();
        let source =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("docs/book/src/datalinks/pendant");
        for f in ["manifest.json", "shapes.ttl", "export.nt"] {
            std::fs::copy(source.join(f), p.join("qpack").join(f)).unwrap();
        }
        let mut shapes = std::fs::read_to_string(p.join("qpack/shapes.ttl")).unwrap();
        shapes.push_str(&format!("\n<urn:test:effect> a sh:NodeShape ; sh:targetClass <{NS}Ability> ; sh:property [ sh:path <{NS}effectText> ; sh:maxCount 1 ] .\n"));
        std::fs::write(p.join("qpack/shapes.ttl"), shapes).unwrap();
        std::fs::write(p.join(".gitattributes"),"qpack/export.nt merge=quipu\nqpack/shapes.ttl merge=quipu\nqpack/manifest.json merge=quipu\n").unwrap();
        rehash(p);
        g(p, &["add", "."]);
        g(p, &["commit", "-m", "base"]);
        let base = g(p, &["rev-parse", "HEAD"]);
        let mut commits = Vec::new();
        for side in ["ours", "theirs"] {
            g(p, &["checkout", "-b", side, &base]);
            let path = p.join("qpack/export.nt");
            let graph = std::fs::read_to_string(&path).unwrap();
            let changed = if kind == "functional" {
                graph
                    .lines()
                    .map(|l| {
                        if l.starts_with(&format!("<{NS}ability/aaa-tracking> <{NS}effectText>")) {
                            format!("<{NS}ability/aaa-tracking> <{NS}effectText> \"{side}\" .\n")
                        } else {
                            format!("{l}\n")
                        }
                    })
                    .collect::<String>()
            } else {
                let iri = if side == "ours" { "aaa-new" } else { "zzz-new" };
                let label = if kind == "alias" {
                    "Shared ability"
                } else if side == "ours" {
                    "Quantum kite"
                } else {
                    "Velvet moon"
                };
                format!(
                    "{graph}<{NS}ability/{iri}> <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <{NS}Ability> .\n<{NS}ability/{iri}> <http://www.w3.org/2000/01/rdf-schema#label> \"{label}\" .\n"
                )
            };
            std::fs::write(path, changed).unwrap();
            rehash(p);
            g(p, &["add", "."]);
            g(p, &["commit", "-m", side]);
            commits.push(g(p, &["rev-parse", "HEAD"]));
        }
        g(p, &["checkout", "ours"]);
        Self {
            temp,
            base,
            ours: commits[0].clone(),
            theirs: commits[1].clone(),
        }
    }
    pub fn merge(&self) -> Output {
        cli(self.path(), &["git-merge", &self.theirs])
    }
    pub fn resolve(&self, key: &str, choice: &str) -> Output {
        cli(
            self.path(),
            &[
                "pendant-resolve",
                &self.base,
                &self.ours,
                &self.theirs,
                "qpack",
                key,
                choice,
            ],
        )
    }
    pub fn commit(&self) {
        g(self.path(), &["add", "."]);
        g(self.path(), &["commit", "-m", "resolved"]);
    }
    pub fn check(&self) -> Output {
        cli(
            self.path(),
            &[
                "pendant-check",
                &self.base,
                &self.ours,
                &self.theirs,
                "HEAD",
            ],
        )
    }
    pub fn graph(&self) -> String {
        std::fs::read_to_string(self.path().join("qpack/export.nt")).unwrap()
    }
    pub fn decisions(&self) -> Value {
        serde_json::from_slice(&std::fs::read(self.path().join("qpack/decisions.json")).unwrap())
            .unwrap()
    }
}
