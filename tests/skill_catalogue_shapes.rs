#![cfg(feature = "shacl")]

const SHAPES: &str = include_str!("../shapes/skill-catalogue.ttl");

fn fixture() -> String {
    let mut data = String::from(
        "@prefix s: <urn:quipu:skills:> .\n<urn:profile:example> a s:Profile ; s:publicOnly true ; s:harness \"claude\", \"codex\" ; s:pin <urn:pin:one>, <urn:pin:two>, <urn:pin:three> .\n",
    );
    for (name, digit) in [("one", "1"), ("two", "2"), ("three", "3")] {
        let hash = digit.repeat(64);
        data.push_str(&format!(
            r#"
<urn:release:{name}> a s:Release ; s:skill <urn:skill:{name}> ;
 s:name "{name}" ; s:description "Example trigger" ;
 s:repository <https://example.org/tools> ; s:sourcePath "skills/{name}/SKILL.md" ;
 s:version "1.0.0" ; s:shareId "sha256:{hash}" ; s:renderSpec <urn:render:{name}> ;
 s:scope "public" ; s:harness "claude", "codex" .
<urn:pin:{name}> a s:Pin ; s:skill <urn:skill:{name}> ;
 s:version "1.0.0" ; s:shareId "sha256:{hash}" .
"#
        ));
    }
    data
}

fn valid(data: &str) -> bool {
    quipu::validate_shapes(SHAPES, data).unwrap().conforms
}

#[test]
fn three_pins_for_both_harnesses_conform() {
    assert!(valid(&fixture()));
}

#[test]
fn missing_release_metadata_refuses() {
    for field in [
        "s:skill <urn:skill:one> ;",
        "s:name \"one\" ;",
        "s:description \"Example trigger\" ;",
        "s:repository <https://example.org/tools> ;",
        "s:sourcePath \"skills/one/SKILL.md\" ;",
        "s:version \"1.0.0\" ;",
        "s:renderSpec <urn:render:one> ;",
        "s:scope \"public\" ;",
    ] {
        let bad = fixture().replacen(field, "", 1);
        assert!(!valid(&bad), "missing {field}");
    }
}

#[test]
fn malformed_digest_scope_harness_and_pin_refuse() {
    for (from, to) in [
        ("sha256:", "SHA256:"),
        ("sha256:", ""),
        ("s:scope \"public\"", "s:scope \"unrestricted\""),
        ("s:harness \"claude\", \"codex\"", "s:harness \"other\""),
        ("s:name \"one\"", "s:name \"../one\""),
        ("a s:Pin ; s:skill <urn:skill:one> ;", "a s:Pin ;"),
        (
            "s:pin <urn:pin:one>, <urn:pin:two>, <urn:pin:three>",
            "s:pin <urn:pin:absent>",
        ),
        ("s:publicOnly true ;", ""),
    ] {
        assert!(!valid(&fixture().replacen(from, to, 1)), "{from} -> {to}");
    }
    assert!(!valid(
        "@prefix s: <urn:quipu:skills:> . <urn:p> a s:Profile ; s:publicOnly true ; s:harness \"codex\" ."
    ));
}

#[test]
fn declarations_do_not_upgrade_legacy_skill_observations() {
    assert!(valid("<urn:old> a <urn:example:Skill> ."));
    assert!(!valid("<urn:old> a <urn:quipu:skills:Release> ."));
    assert!(valid(
        &fixture().replace("s:scope \"public\"", "s:scope \"internal\"")
    ));
    // Cross-record public-only admission is the selector's job, not a false
    // claim that SHACL can see the consumer's selected dataset or authority.
}
