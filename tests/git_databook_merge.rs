//! Real Git inputs, lossless Markdown and byte-for-byte replay; no store.
#![cfg(feature = "shacl")]
#[path = "support/git_pendant_fixture.rs"]
mod fixture;
use fixture::{cli, g};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

const BOOK: &str = "people.databook.md";
fn document(value: &str) -> String {
    format!(
        "---\nid: urn:book\ntitle: People\ntype: databook\nversion: 1.5.0\nunknown: keep-me\n---\n\n# Human prose stays byte-for-byte.\n\n<!-- databook:id: data -->\n<!-- databook:graph: urn:g -->\n```turtle\n<urn:s> a <urn:Person>; <urn:name> \"{value}\" .\n```\n\n<!-- databook:id: constraints -->\n```shacl\n@prefix sh: <http://www.w3.org/ns/shacl#> .\n<urn:shape> a sh:NodeShape; sh:targetClass <urn:Person>; sh:property [sh:path <urn:name>; sh:maxCount 1] .\n```\n\nTail prose.\n"
    )
}
struct Book {
    temp: tempfile::TempDir,
    refs: [String; 3],
}
impl Book {
    fn new(ours: &str, theirs: &str) -> Self {
        Self::from_sources(&document("base"), ours, theirs)
    }
    fn from_sources(base_source: &str, ours: &str, theirs: &str) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let p = temp.path();
        g(p, &["init", "-b", "base"]);
        g(p, &["config", "user.name", "DataBook test"]);
        g(p, &["config", "user.email", "test@example.org"]);
        std::fs::write(
            p.join(".gitattributes"),
            format!("{BOOK} merge=quipu-databook\n"),
        )
        .unwrap();
        std::fs::write(p.join(BOOK), base_source).unwrap();
        g(p, &["add", "."]);
        g(p, &["commit", "-m", "base"]);
        let base = g(p, &["rev-parse", "HEAD"]);
        let mut commits = vec![base.clone()];
        for (branch, source) in [("ours", ours), ("theirs", theirs)] {
            g(p, &["checkout", "-b", branch, &base]);
            std::fs::write(p.join(BOOK), source).unwrap();
            g(p, &["add", "."]);
            g(p, &["commit", "--allow-empty", "-m", branch]);
            commits.push(g(p, &["rev-parse", "HEAD"]));
        }
        g(p, &["checkout", "ours"]);
        Self {
            temp,
            refs: commits.try_into().unwrap(),
        }
    }
    fn p(&self) -> &Path {
        self.temp.path()
    }
    fn merge(&self) -> Output {
        cli(self.p(), &["git-merge", "theirs"])
    }
    fn source(&self) -> String {
        std::fs::read_to_string(self.p().join(BOOK)).unwrap()
    }
    fn commit(&self) -> String {
        g(self.p(), &["add", "."]);
        g(self.p(), &["commit", "-m", "merge"]);
        g(self.p(), &["rev-parse", "HEAD"])
    }
    fn check(&self, result: &str) -> Output {
        cli(
            self.p(),
            &[
                "databook-check",
                &self.refs[0],
                &self.refs[1],
                &self.refs[2],
                result,
                BOOK,
            ],
        )
    }
}
fn tool(book: &Book, input: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_quipu"))
        .current_dir(book.p())
        .args(["git-mergetool", BOOK, "--by", "reviewer"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn functional_conflict_shared_show_resolve_and_immutable_replay() {
    let book = Book::new(&document("ours"), &document("theirs"));
    let merged = book.merge();
    assert_eq!(
        merged.status.code(),
        Some(2),
        "{}",
        String::from_utf8_lossy(&merged.stderr)
    );
    let path = format!("{BOOK}.decisions.json");
    let show = cli(book.p(), &["merge", "show", &path]);
    assert!(
        show.status.success(),
        "{}",
        String::from_utf8_lossy(&show.stderr)
    );
    assert!(String::from_utf8_lossy(&show.stdout).contains("graph:0/conflict:0"));
    let resolved = cli(
        book.p(),
        &[
            "merge",
            "resolve",
            &path,
            "graph:0/conflict:0",
            "--choose",
            "theirs",
            "--by",
            "reviewer",
        ],
    );
    assert!(
        resolved.status.success(),
        "{}",
        String::from_utf8_lossy(&resolved.stderr)
    );
    assert!(book.source().contains("\"theirs\""));
    assert!(book.source().contains("unknown: keep-me"));
    assert!(book.source().ends_with("Tail prose.\n"));
    let result = book.commit();
    let checked = book.check(&result);
    assert!(
        checked.status.success(),
        "{}",
        String::from_utf8_lossy(&checked.stderr)
    );
    std::fs::write(
        book.p().join(BOOK),
        book.source().replace("Tail prose.", "forged"),
    )
    .unwrap();
    let forged = book.commit();
    assert!(!book.check(&forged).status.success());
}

#[test]
fn disjoint_slots_preserve_opaque_envelope_and_both_edits() {
    let ours = document("base").replace("# Human prose", "# Edited human prose");
    let theirs = document("theirs");
    let book = Book::new(&ours, &theirs);
    let merged = book.merge();
    assert!(
        merged.status.success(),
        "{}",
        String::from_utf8_lossy(&merged.stderr)
    );
    assert_eq!(
        book.source(),
        theirs.replace("# Human prose", "# Edited human prose")
    );
    assert!(book.check(&book.commit()).status.success());
}

#[test]
fn eof_unoffered_and_stale_sidecars_do_not_rewrite() {
    let book = Book::new(&document("ours"), &document("theirs"));
    assert_eq!(book.merge().status.code(), Some(2));
    let before = book.source();
    for input in ["", "quit\n", "graph:0/conflict:0 both\n"] {
        assert_eq!(tool(&book, input).status.code(), Some(2));
        assert_eq!(book.source(), before);
    }
    let manual = before.replace("Tail prose.", "Manual alternative.");
    std::fs::write(book.p().join(BOOK), &manual).unwrap();
    assert_eq!(
        tool(&book, "graph:0/conflict:0 ours\n").status.code(),
        Some(1)
    );
    assert_eq!(book.source(), manual);
    std::fs::write(book.p().join(BOOK), &before).unwrap();
    let path = book.p().join(format!("{BOOK}.decisions.json"));
    let mut file: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    file["inputs"][0] = "forged".into();
    std::fs::write(path, serde_json::to_vec(&file).unwrap()).unwrap();
    assert_eq!(
        tool(&book, "graph:0/conflict:0 ours\n").status.code(),
        Some(1)
    );
    assert_eq!(book.source(), before);
}

#[test]
fn invalid_hash_blank_nodes_and_opaque_overlap_refuse_before_git_changes() {
    let invalid = [
        document("ours").replace("<urn:s>", "_:unbound"),
        document("ours").replace(
            "<!-- databook:id: data -->",
            "<!-- databook:id: data -->\n<!-- quipu:sha256: wrong -->",
        ),
        document("ours").replace("Tail prose.", "ours prose."),
    ];
    for ours in invalid {
        let book = Book::new(
            &ours,
            &document("theirs").replace("Tail prose.", "theirs prose."),
        );
        assert!(!book.merge().status.success());
        assert_eq!(book.source(), ours);
        assert!(!book.p().join(".git/MERGE_HEAD").exists());
    }
}

#[test]
fn real_git_mergetool_stages_a_reviewed_document_and_replay_checks_it() {
    let book = Book::new(&document("ours"), &document("theirs"));
    assert_eq!(book.merge().status.code(), Some(2));
    let exe = env!("CARGO_BIN_EXE_quipu").replace('\'', "'\\''");
    let command = format!("'{exe}' git-mergetool \"$MERGED\" --by reviewer");
    let mut child = Command::new("git")
        .current_dir(book.p())
        .args([
            "-c",
            &format!("mergetool.quipu.cmd={command}"),
            "-c",
            "mergetool.quipu.trustExitCode=true",
            "-c",
            "mergetool.keepBackup=false",
            "mergetool",
            "--no-prompt",
            "--tool=quipu",
            "--",
            BOOK,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"graph:0/conflict:0 theirs\n")
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(g(book.p(), &["ls-files", "-u", "--", BOOK]).is_empty());
    assert!(book.check(&book.commit()).status.success());
}

#[test]
fn graph_identity_changes_refuse_even_when_only_one_side_changes_the_block() {
    let ours = document("base").replace("databook:graph: urn:g", "databook:graph: urn:other");
    let theirs = document("base").replace("Tail prose.", "Other prose.");
    let book = Book::new(&ours, &theirs);
    assert!(!book.merge().status.success());
    assert_eq!(book.source(), ours);
    assert!(!book.p().join(".git/MERGE_HEAD").exists());
}

#[test]
fn document_signature_metadata_cannot_be_carried_onto_new_bytes() {
    let signed = |value: &str| {
        document(value).replace(
            "unknown: keep-me",
            "unknown: keep-me\nsignature: original-input-signature",
        )
    };
    let ours = signed("ours");
    let book = Book::from_sources(&signed("base"), &ours, &signed("theirs"));
    let output = book.merge();
    assert!(!output.status.success());
    assert_eq!(book.source(), ours);
    assert!(!book.p().join(".git/MERGE_HEAD").exists());
}

#[test]
fn prose_that_looks_like_yaml_counts_remains_opaque() {
    let suffix = "\ngraph:\n  triple_count: this is prose\n  subjects: keep these bytes\n";
    let ours = format!("{}{suffix}", document("ours"));
    let theirs = format!("{}{suffix}", document("theirs"));
    // Give the base the same prose: the fixture's base ends at Tail prose.
    let book = Book::new(&ours, &theirs);
    assert_eq!(book.merge().status.code(), Some(2));
    let output = tool(&book, "graph:0/conflict:0 ours\n");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(book.source().ends_with(suffix));
    assert!(book.check(&book.commit()).status.success());
}

fn derived(value: &str) -> String {
    use sha2::{Digest, Sha256};
    let source = document(value);
    let parsed = quipu::databook::parse(&source).unwrap();
    let hash = hex::encode(Sha256::digest(parsed.blocks[0].payload.as_bytes()));
    source
        .replace(
            "id: urn:book",
            &format!("id: urn:quipu:databook:sha256:{hash}"),
        )
        .replace(
            "unknown: keep-me",
            "unknown: keep-me\ngraph:\n  triple_count: 2\n  subjects: 1",
        )
        .replace(
            "<!-- databook:id: data -->",
            &format!("<!-- databook:id: data -->\n<!-- quipu:sha256: {hash} -->"),
        )
}

#[test]
fn exporter_profile_hash_and_derived_identity_follow_reviewed_payload() {
    let book = Book::from_sources(&derived("base"), &derived("ours"), &derived("theirs"));
    assert_eq!(book.merge().status.code(), Some(2));
    let output = tool(&book, "graph:0/conflict:0 theirs\n");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(book.source(), derived("theirs"));
    assert!(book.check(&book.commit()).status.success());
}

#[test]
fn equal_subjects_in_distinct_graphs_do_not_share_a_slot() {
    let other = "\n<!-- databook:id: other -->\n<!-- databook:graph: urn:h -->\n```turtle\n<urn:s> a <urn:Person>; <urn:name> \"other\" .\n```\n";
    let base = format!("{}{other}", document("base"));
    let ours = format!("{}{other}", document("ours"));
    let theirs = base.replace("\"other\"", "\"changed other\"");
    let book = Book::from_sources(&base, &ours, &theirs);
    let merged = book.merge();
    assert!(
        merged.status.success(),
        "{}",
        String::from_utf8_lossy(&merged.stderr)
    );
    assert!(book.source().contains("\"ours\""));
    assert!(book.source().contains("\"changed other\""));
    assert!(book.check(&book.commit()).status.success());
}

#[test]
fn block_addition_and_delete_modify_have_distinct_verdicts() {
    let extra = "\n<!-- databook:id: extra -->\n<!-- databook:graph: urn:g -->\n```turtle\n<urn:extra> <urn:p> \"kept\" .\n```\n";
    let base = document("base");
    let ours = format!("{base}{extra}");
    let book = Book::from_sources(&base, &ours, &document("theirs"));
    let merged = book.merge();
    assert!(
        merged.status.success(),
        "{}",
        String::from_utf8_lossy(&merged.stderr)
    );
    assert!(book.source().ends_with(extra));
    assert!(book.source().contains("\"theirs\""));
    assert!(book.check(&book.commit()).status.success());

    let base = format!("{base}{extra}");
    let ours = document("base");
    let theirs = base.replace("\"kept\"", "\"modified\"");
    let book = Book::from_sources(&base, &ours, &theirs);
    assert!(!book.merge().status.success());
    assert_eq!(book.source(), ours);
    assert!(!book.p().join(".git/MERGE_HEAD").exists());
}

#[test]
fn crlf_is_preserved_and_lossy_parser_reserialization_is_a_negative_control() {
    let base = document("base").replace('\n', "\r\n");
    let ours = document("ours").replace('\n', "\r\n");
    let theirs = document("theirs").replace('\n', "\r\n");
    let book = Book::from_sources(&base, &ours, &theirs);
    assert_eq!(book.merge().status.code(), Some(2));
    assert!(tool(&book, "graph:0/conflict:0 ours\n").status.success());
    assert_eq!(book.source(), ours);
    assert!(book.check(&book.commit()).status.success());

    let parsed = quipu::databook::parse(&ours).unwrap();
    let mut lossy = format!(
        "---\nid: {}\ntitle: {}\ntype: {}\nversion: {}\n---\n",
        parsed.header.id, parsed.header.title, parsed.header.doc_type, parsed.header.version
    );
    for block in parsed.blocks {
        lossy.push_str(&quipu::databook::fence(&block.label, &[], &block.payload).unwrap());
    }
    assert!(quipu::databook::parse(&lossy).is_ok());
    let preserves = |source: &str| {
        source.contains("unknown: keep-me")
            && source.contains("# Human prose stays byte-for-byte.")
            && source.ends_with("Tail prose.\r\n")
    };
    assert!(preserves(&book.source()));
    assert!(
        !preserves(&lossy),
        "the naive parser/fence roundtrip must fail the preservation contract"
    );
}

#[test]
fn shacl_failure_leaves_document_and_decisions_unchanged() {
    let constrained =
        |value: &str| document(value).replace("sh:maxCount 1", "sh:maxCount 1; sh:minLength 4");
    let book = Book::from_sources(
        &constrained("base"),
        &constrained("ours"),
        &constrained("x"),
    );
    assert_eq!(book.merge().status.code(), Some(2));
    let path = book.p().join(format!("{BOOK}.decisions.json"));
    let source = book.source();
    let decisions = std::fs::read(&path).unwrap();
    let invalid = tool(&book, "graph:0/conflict:0 theirs\n");
    assert_eq!(invalid.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&invalid.stderr).contains("SHACL"));
    assert_eq!(book.source(), source);
    assert_eq!(std::fs::read(path).unwrap(), decisions);
    assert!(tool(&book, "graph:0/conflict:0 ours\n").status.success());
    assert!(book.check(&book.commit()).status.success());
}

#[test]
fn accepted_alias_stays_in_its_unique_data_block_and_replays() {
    let append = |side: &str| {
        document("base").replace("<urn:s> a <urn:Person>; <urn:name> \"base\" .", &format!("<urn:s> a <urn:Person>; <urn:name> \"base\" .\n<urn:{side}> a <urn:Person>; <http://www.w3.org/2000/01/rdf-schema#label> \"Maya\" ."))
    };
    let book = Book::new(&append("new-ours"), &append("new-theirs"));
    assert_eq!(book.merge().status.code(), Some(2));
    let output = tool(&book, "graph:0/alias:0 accept\n");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        book.source()
            .contains("http://www.w3.org/2002/07/owl#sameAs")
    );
    assert!(book.check(&book.commit()).status.success());
}
