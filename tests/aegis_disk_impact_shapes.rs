#![cfg(feature = "shacl")]

const SHAPES: &str = include_str!("../shapes/aegis-ontology.shapes.ttl");

fn disk_impact_fixture(
    signature: &str,
    filesystem: &str,
    delta: &str,
    observed_at: &str,
) -> String {
    format!(
        r#"
            @prefix aegis: <http://aegis.gastown.local/ontology/> .
            @prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
            @prefix xsd: <http://www.w3.org/2001/XMLSchema#> .
            aegis:test-impact a aegis:CommandDiskImpactObservation ;
                rdfs:label "cargo build disk impact" ;
                aegis:commandSignature {signature} ;
                aegis:filesystemIdentity {filesystem} ;
                aegis:diskDeltaBytes {delta} ;
                aegis:observedAt {observed_at} .
        "#
    )
}

#[test]
fn command_disk_impact_accepts_consumed_and_freed_space_samples() {
    for delta in ["\"1048576\"^^xsd:integer", "\"-4096\"^^xsd:integer"] {
        let data = disk_impact_fixture(
            "\"cargo:build|repo:rust-project|cwd:repo-root\"",
            "\"root:primary\"",
            delta,
            "\"2026-09-02T20:00:00Z\"^^xsd:dateTime",
        );
        assert!(quipu::validate_shapes(SHAPES, &data).unwrap().conforms);
    }
}

#[test]
fn command_disk_impact_rejects_raw_argv_paths_and_wrong_datatypes() {
    let cases = [
        disk_impact_fixture(
            "\"cargo build --release|repo:rust-project|cwd:/workspace/repo\"",
            "\"root:primary\"",
            "\"12\"^^xsd:integer",
            "\"2026-09-02T20:00:00Z\"^^xsd:dateTime",
        ),
        disk_impact_fixture(
            "\"cargo:build|repo:rust-project|cwd:repo-root\"",
            "\"/\"",
            "\"12.5\"^^xsd:decimal",
            "\"not-a-date\"",
        ),
    ];
    for data in cases {
        assert!(!quipu::validate_shapes(SHAPES, &data).unwrap().conforms);
    }
}

#[test]
fn command_disk_impact_requires_each_raw_sample_field() {
    let complete = disk_impact_fixture(
        "\"cargo:build|repo:rust-project|cwd:repo-root\"",
        "\"root:primary\"",
        "\"12\"^^xsd:integer",
        "\"2026-09-02T20:00:00Z\"^^xsd:dateTime",
    );
    for predicate in [
        "aegis:commandSignature",
        "aegis:filesystemIdentity",
        "aegis:diskDeltaBytes",
        "aegis:observedAt",
    ] {
        let without = complete
            .lines()
            .filter(|line| !line.contains(predicate))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(!quipu::validate_shapes(SHAPES, &without).unwrap().conforms);
    }
}

#[test]
fn disk_aggregate_summary_fields_are_signed_bounded_and_migration_safe() {
    let fixture = disk_impact_fixture(
        "\"cargo:build|repo:all|cwd:all\"",
        "\"root:primary\"",
        "\"-4096\"^^xsd:integer",
        "\"2026-11-06T00:00:00Z\"^^xsd:dateTime",
    );
    let summary = format!(
        "{fixture}\naegis:test-impact aegis:commandClass \"cargo:build\" ; aegis:aggregateState \"bounded-json-state\" ; aegis:runCount \"5\"^^xsd:integer ; aegis:maximumDeltaBytes \"1024\"^^xsd:integer ; aegis:p95DeltaBytes \"-4096\"^^xsd:integer ."
    );
    assert!(quipu::validate_shapes(SHAPES, &summary).unwrap().conforms);
    for bad in [
        summary.replace("\"5\"^^xsd:integer", "\"0\"^^xsd:integer"),
        summary.replace("\"cargo:build\"", "\"cargo:build /private/operand\""),
        summary.replace("bounded-json-state", &"x".repeat(32769)),
        format!("{summary}\naegis:test-impact aegis:runCount \"6\"^^xsd:integer ."),
    ] {
        assert!(!quipu::validate_shapes(SHAPES, &bad).unwrap().conforms);
    }
    // Legacy fixtures remain valid; no raw-run migration is implied.
    assert!(quipu::validate_shapes(SHAPES, &fixture).unwrap().conforms);
}
