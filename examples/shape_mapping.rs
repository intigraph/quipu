//! Master-only C1 lifecycle demonstration using an isolated in-memory store.
use quipu::shape_mapping::catalog::RowDecision;
use quipu::shape_mapping::{Correspondence, RegistryKind, ShapeMapping};
use quipu::{Result, Store};
use serde_json::json;

fn main() -> Result<()> {
    let mut store = Store::open_in_memory()?;
    for graph in [
        "urn:demo:source",
        "urn:demo:target",
        "urn:demo:mappings",
        "urn:quipu:graph:operations",
    ] {
        store.graph_create(graph)?;
    }
    store.load_shapes(
        "tickets",
        "<urn:demo:Ticket> <urn:demo:version> \"one\" .",
        "2026-10-25T00:00:00Z",
    )?;
    let id = "urn:demo:mapping:tickets";
    println!(
        "{}",
        json!({"stage":"before","mapping":store.shape_mapping_receipt(id)?})
    );
    let row = Correspondence {
        subject_id: "urn:demo:Ticket".into(),
        predicate_id: "http://www.w3.org/2002/07/owl#equivalentClass".into(),
        object_id: "urn:demo:Task".into(),
        mapping_justification: "semapv:ManualMappingCuration".into(),
        author_id: String::new(),
        quipu_review: String::new(),
    };
    let mapping = ShapeMapping {
        id: id.into(),
        graph: "urn:demo:mappings".into(),
        maps_from: ["urn:demo:source".into()].into(),
        maps_to: ["urn:demo:target".into()].into(),
        pins: store
            .current_shape_pins(RegistryKind::Shapes, "tickets")?
            .into_iter()
            .collect(),
        rows: [row.clone()].into(),
    };
    let before_preparation = store.latest_tx_id()?;
    let proposal = store.propose_shape_mapping(mapping)?;
    let decision = proposal.decide(
        "urn:demo:reviewer",
        vec![RowDecision {
            subject_id: row.subject_id,
            predicate_id: row.predicate_id,
            object_id: row.object_id,
            accept: true,
        }],
    )?;
    assert_eq!(before_preparation, store.latest_tx_id()?);
    let applied = store.apply_shape_mapping(
        decision.mapping.clone(),
        &decision.expected_digest,
        "urn:demo:reviewer",
        None,
    )?;
    assert_eq!(applied.status, "decided");
    assert!(applied.assert_writable["urn:demo:Task"]);
    println!("{}", json!({"stage":"applied","receipt":applied}));
    store.load_shapes(
        "tickets",
        "<urn:demo:Ticket> <urn:demo:version> \"two\" .",
        "2026-10-26T00:00:00Z",
    )?;
    let flagged = store
        .shape_mapping_receipt(id)?
        .expect("mapping must remain visible");
    assert_eq!(flagged.status, "flagged");
    assert!(!flagged.assert_writable["urn:demo:Task"]);
    assert!(
        store
            .apply_shape_mapping(
                decision.mapping,
                &decision.expected_digest,
                "urn:demo:reviewer",
                None
            )
            .is_err()
    );
    println!(
        "{}",
        json!({"stage":"shape_changed","receipt":flagged,"old_pin_apply":"refused"})
    );
    Ok(())
}
