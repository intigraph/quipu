use super::*;

const SHAPES: &str =
    "@prefix sh: <http://www.w3.org/ns/shacl#> . [] sh:path <urn:p>; sh:maxCount 1 .";
const BASE: &str = "<urn:s> <urn:p> \"base\"";
const LOCAL: &str = "<urn:s> <urn:p> \"local\"";
const NOTE: &str = "<urn:s> <urn:note> \"upstream\"";

fn journal(tx: i64, triples: &[&str]) -> BaseJournal {
    BaseJournal::new(
        "urn:store:preview".into(),
        "urn:epoch:preview".into(),
        tx,
        "ROOT".into(),
        digest(SHAPES.as_bytes()),
        triples.iter().map(|t| BaseRow {
            triple: (*t).into(),
            valid_from: "2026-01-01T00:00:00Z".into(),
            valid_to: None,
        }),
    )
    .unwrap()
}

fn local() -> ClientOverlay {
    let mut local = ClientOverlay::open(journal(3, &[BASE]), "urn:fork:preview".into()).unwrap();
    local.replace([LOCAL.into()]).unwrap();
    local
}

#[test]
fn composed_read_keeps_canonical_catchup_and_private_edit_without_mutation() {
    let local = local();
    let before = serde_json::to_vec(&local).unwrap();
    let current = journal(4, &[BASE, NOTE]);
    let OverlayPreview::Composed { triples } = local.preview(&current, SHAPES).unwrap() else {
        panic!("unrelated canonical addition must compose");
    };
    assert_eq!(triples, [LOCAL.into(), NOTE.into()].into());
    assert_eq!(before, serde_json::to_vec(&local).unwrap());
    assert_eq!(current, journal(4, &[BASE, NOTE]));
    assert_eq!(local.base().base_tx(), 3);
}

#[test]
fn composed_read_returns_conflict_instead_of_partial_graph_or_local_overwrite() {
    let local = local();
    let before = serde_json::to_vec(&local).unwrap();
    for values in [vec!["<urn:s> <urn:p> \"master\"", NOTE], vec![NOTE]] {
        let OverlayPreview::Conflicts { records } =
            local.preview(&journal(4, &values), SHAPES).unwrap()
        else {
            panic!("replacement and delete-versus-replace must conflict");
        };
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].predicate, "urn:p");
        assert_eq!(records[0].theirs, vec!["\"local\""]);
        assert_eq!(before, serde_json::to_vec(&local).unwrap());
    }
}

#[test]
fn composed_read_refuses_foreign_or_changed_bindings_and_temporal_context() {
    let local = local();
    let original = journal(4, &[BASE, NOTE]);
    for field in ["store_id", "log_epoch", "graph", "shapes_hash", "base_tx"] {
        let mut value = serde_json::to_value(&original).unwrap();
        value[field] = if field == "base_tx" {
            serde_json::json!(2)
        } else {
            serde_json::json!("urn:foreign")
        };
        let current: BaseJournal = serde_json::from_value(value).unwrap();
        assert!(local.preview(&current, SHAPES).is_err(), "binding {field}");
    }
    assert!(local.preview(&journal(3, &[BASE, NOTE]), SHAPES).is_err());
    assert!(local.preview(&original, "").is_err());
    assert!(local.preview(&original, "not valid Turtle").is_err());
    let mut value = serde_json::to_value(&original).unwrap();
    value["rows"][0]["valid_to"] = serde_json::json!("2026-02-01T00:00:00Z");
    let current: BaseJournal = serde_json::from_value(value).unwrap();
    assert!(local.preview(&current, SHAPES).is_err());
}

#[test]
fn composed_read_agreeing_edits_and_unchanged_canonical_do_not_conflict() {
    let local = local();
    for current in [journal(3, &[BASE]), journal(4, &[LOCAL])] {
        let OverlayPreview::Composed { triples } = local.preview(&current, SHAPES).unwrap() else {
            panic!("agreeing or unchanged side must compose");
        };
        assert_eq!(triples, [LOCAL.into()].into());
    }
}
