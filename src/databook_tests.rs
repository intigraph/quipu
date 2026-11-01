use super::*;

/// `examples/minimal.databook.md` from kurtcagle/databook @ 27d014a5.
const MINIMAL: &str = r#"---
id: https://w3id.org/databook/examples/minimal-v1
title: "Minimal DataBook"
type: databook
version: 1.0.0
created: 2026-04-25
process:
  transformer: human
  transformer_type: human
  inputs: []
  timestamp: 2026-04-25T12:00:00Z
  agent:
    name: Kurt Cagle
    iri: https://holongraph.com/people/kurt-cagle
    role: orchestrator
---

# Minimal DataBook

This is the smallest valid DataBook: five required identity fields, a process stamp, and one fenced data block.

```turtle
<!-- databook:id: primary-block -->
@prefix ex: <https://example.org/> .
ex:Thing a ex:Class ;
    ex:label "A thing" .
```
"#;

fn book(front: &str, body: &str) -> String {
    format!("---\n{front}---\n\n{body}")
}

const IDENTITY: &str = "id: urn:x\ntitle: T\ntype: databook\nversion: 1.0.0\ncreated: 2026-11-01\nprocess:\n  transformer: t\n";

#[test]
fn reads_the_spec_minimal_example() {
    let db = parse(MINIMAL).unwrap();
    assert_eq!(db.form, FrontmatterForm::Canonical);
    assert_eq!(
        db.header.id,
        "https://w3id.org/databook/examples/minimal-v1"
    );
    assert_eq!(db.header.title, "Minimal DataBook");
    assert_eq!(db.header.version, "1.0.0");
    assert_eq!(db.header.created.as_deref(), Some("2026-04-25"));
    assert!(db.header.has_process);
    assert!(db.warnings.is_empty(), "{:?}", db.warnings);
    assert_eq!(db.blocks.len(), 1);
    let block = &db.blocks[0];
    assert_eq!(block.id(), Some("primary-block"));
    assert_eq!(block.kind(), BlockKind::Rdf(RdfFormat::Turtle));
    assert!(
        block.payload.starts_with("@prefix ex:"),
        "inline annotation stripped"
    );
    assert!(!block.payload.contains("databook:id"));
}

#[test]
fn reads_the_legacy_script_frontmatter() {
    let text = format!(
        "<script language=\"application/yaml\">\n\n---\n{IDENTITY}---\n\n</script>\n\n```turtle\n<urn:a> <urn:b> <urn:c> .\n```\n"
    );
    let db = parse(&text).unwrap();
    assert_eq!(db.form, FrontmatterForm::Script);
    assert_eq!(db.header.id, "urn:x");
    assert_eq!(db.blocks.len(), 1);
}

#[test]
fn pre_fence_annotations_win_and_directives_are_read() {
    let body = "<!-- databook:id: outer -->\n<!-- holon:layer: domain -->\n<!-- mode=executed cache=true -->\n\n```turtle\n<!-- databook:id: inner -->\n<!-- databook:graph: urn:g -->\n<urn:a> <urn:b> <urn:c> .\n```\n";
    let db = parse(&book(IDENTITY, body)).unwrap();
    let block = &db.blocks[0];
    assert_eq!(block.id(), Some("outer"), "pre-fence wins over inline");
    assert_eq!(
        block.meta["graph"], "urn:g",
        "inline keys not set pre-fence still apply"
    );
    assert_eq!(
        block.meta["holon:layer"], "domain",
        "profile prefixes keep their prefix"
    );
    assert_eq!(block.directives["mode"], "executed");
    assert_eq!(block.directives["cache"], "true");
    assert_eq!(block.payload, "<urn:a> <urn:b> <urn:c> .");
}

#[test]
fn a_printed_or_display_only_block_is_not_data() {
    let body = "<!-- mode=printed -->\n```turtle\n<urn:a> <urn:b> <urn:c> .\n```\n\n```turtle\n<!-- databook:display-only: true -->\n<urn:a> <urn:b> <urn:c> .\n```\n\n```sparql\nASK {}\n```\n";
    let db = parse(&book(IDENTITY, body)).unwrap();
    let kinds: Vec<_> = db.blocks.iter().map(Block::kind).collect();
    assert_eq!(
        kinds,
        [BlockKind::Display, BlockKind::Display, BlockKind::Query]
    );
}

#[test]
fn labels_map_to_kinds() {
    let kind = |label: &str| {
        Block {
            label: label.into(),
            meta: BTreeMap::new(),
            directives: BTreeMap::new(),
            payload: String::new(),
            line: 1,
        }
        .kind()
    };
    assert_eq!(kind("turtle12"), BlockKind::Rdf(RdfFormat::Turtle));
    assert_eq!(kind("n-triples"), BlockKind::Rdf(RdfFormat::NTriples));
    assert_eq!(kind("trig"), BlockKind::Rdf(RdfFormat::TriG));
    assert_eq!(kind("n-quads"), BlockKind::Rdf(RdfFormat::NQuads));
    assert_eq!(kind("shacl"), BlockKind::Shapes);
    assert_eq!(kind("sparql-update"), BlockKind::Update);
    assert_eq!(kind("encrypted-turtle"), BlockKind::Opaque);
    assert_eq!(kind("json-ld"), BlockKind::Unsupported);
    assert_eq!(kind("python"), BlockKind::Display);
    assert_eq!(kind(""), BlockKind::Display);
}

#[test]
fn a_longer_fence_holds_a_three_backtick_line() {
    let body = "````turtle\n<urn:a> <urn:b> \"\"\"x\n```\ny\"\"\" .\n````\n";
    let db = parse(&book(IDENTITY, body)).unwrap();
    assert_eq!(db.blocks.len(), 1);
    assert!(db.blocks[0].payload.contains("\n```\n"));
}

#[test]
fn bom_and_crlf_are_normalised() {
    let text = format!("\u{feff}{}", MINIMAL.replace('\n', "\r\n"));
    let db = parse(&text).unwrap();
    assert_eq!(db.header.title, "Minimal DataBook");
    assert!(!db.blocks[0].payload.contains('\r'));
}

#[test]
fn spec_errors_refuse_the_document() {
    let data = "```turtle\n<urn:a> <urn:b> <urn:c> .\n```\n";
    let err = |text: &str| parse(text).unwrap_err().to_string();
    assert!(err("# just markdown\n").contains("no YAML frontmatter"));
    assert!(err("---\nid: [unclosed\n---\n").contains("malformed YAML"));
    assert!(err("---\n- a list\n---\n").contains("not a YAML mapping"));
    assert!(err(&book("id: urn:x\ntype: databook\nversion: 1\n", data)).contains("`title`"));
    assert!(
        err(&book(
            "id: urn:x\ntitle: T\ntype: notebook\nversion: 1\n",
            data
        ))
        .contains("not a DataBook type")
    );
    assert!(err(&book(IDENTITY, "```python\nprint(1)\n```\n")).contains("no fenced data block"));
    assert!(
        err(&book(IDENTITY, "```turtle\n<urn:a> <urn:b> <urn:c> .\n")).contains("never closed")
    );
    assert!(err("---\nid: urn:x\n").contains("never closed"));
}

#[test]
fn missing_created_and_process_are_warnings() {
    let db = parse(&book(
        "id: urn:x\ntitle: T\ntype: databook\nversion: 2\n",
        "```n-triples\n<urn:a> <urn:b> <urn:c> .\n```\n",
    ))
    .unwrap();
    assert_eq!(db.header.version, "2");
    assert_eq!(db.warnings.len(), 2, "{:?}", db.warnings);
}

#[test]
fn written_frontmatter_cannot_be_broken_out_of() {
    let hostile = "x\"\n---\ntype: bad\n# : [";
    let mut front = Frontmatter::default();
    front
        .text(0, "id", "urn:x")
        .text(0, "title", hostile)
        .text(0, "type", "databook")
        .text(0, "version", "1.0.0")
        .open(0, "graph")
        .number(1, "triple_count", 3)
        .open(0, "process")
        .open(1, "inputs")
        .item(2, "iri", "urn:in")
        .text(3, "role", "primary");
    let text = format!(
        "{}\n```turtle\n<urn:a> <urn:b> <urn:c> .\n```\n",
        front.finish()
    );
    let db = parse(&text).unwrap();
    assert_eq!(db.header.title, hostile);
    assert_eq!(db.header.doc_type, "databook");
    assert_eq!(db.header.triple_count, Some(3));
    assert!(db.header.has_process);
}

#[test]
fn fence_round_trips_and_refuses_what_would_break_it() {
    let block = fence(
        "turtle",
        &[("id", "a"), ("quipu:sha256", "00")],
        "<urn:a> <urn:b> <urn:c> .\n",
    )
    .unwrap();
    assert_eq!(
        block,
        "<!-- databook:id: a -->\n<!-- quipu:sha256: 00 -->\n```turtle\n<urn:a> <urn:b> <urn:c> .\n```\n"
    );
    let db = parse(&book(IDENTITY, &block)).unwrap();
    assert_eq!(db.blocks[0].meta["quipu:sha256"], "00");
    assert!(
        fence("turtle", &[], "x\n```\ny")
            .unwrap_err()
            .to_string()
            .contains("line 2")
    );
    assert!(fence("turtle", &[("id", "a --> b")], "x").is_err());
    assert!(fence("turtle", &[("id", "a\nb")], "x").is_err());
}

#[test]
fn a_repeated_block_id_is_refused() {
    let body = "<!-- databook:id: a -->\n```turtle\n<urn:a> <urn:b> <urn:c> .\n```\n\n```turtle\n<!-- databook:id: a -->\n<urn:a> <urn:b> <urn:d> .\n```\n";
    let err = parse(&book(IDENTITY, body)).unwrap_err().to_string();
    assert!(err.contains("`a` is used by more than one block"), "{err}");
}
