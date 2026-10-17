//! Isolated legacy-schema and request-write provenance demonstration.
use quipu::{
    Datum, Op, Store, Value,
    write_provenance::{RequestProvenance, scoped},
};
use std::sync::Arc;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let db = directory.path().join("demo.db");
    let path = db.to_str().ok_or("non-UTF8 fixture path")?;
    drop(Store::open(path)?);
    // An isolated pre-provenance schema, not a production downgrade.
    rusqlite::Connection::open(path)?.execute_batch(
        "DROP TABLE transaction_provenance; DROP TABLE transaction_provenance_state;",
    )?;
    let legacy = Store::open_read_only(path)?;
    println!(
        "BEFORE legacy read-only schema: {:?}",
        legacy.provenance_state()?
    );
    drop(legacy);
    let mut store = Store::open(path)?;
    let item = Datum {
        entity: store.intern("urn:demo:item")?,
        attribute: store.intern("urn:demo:label")?,
        value: Value::Str("present".into()),
        op: Op::Assert,
        valid_from: "2026-01-01T00:00:00Z".into(),
        valid_to: None,
    };
    let declared = Arc::new(
        RequestProvenance::classify(
            "demo-client",
            "/knot",
            [
                Some("demo-agent"),
                Some("codex"),
                Some("demo-host"),
                Some("demo-session"),
                Some("demo-model"),
            ],
        )
        .with_context(Some("demo-task"), Some("1.0")),
    );
    let tx = scoped(Some(declared.clone()), || {
        store.transact(
            std::slice::from_ref(&item),
            "2026-01-01T00:00:00Z",
            None,
            None,
        )
    })?;
    println!(
        "AFTER request-scoped commit: {}",
        serde_json::to_string(&store.transaction_provenance(tx)?)?
    );
    let noop = scoped(Some(declared), || {
        store.transact(&[item], "2026-01-01T00:00:00Z", None, None)
    })?;
    println!(
        "CONTROL idempotent write declaration: {:?}",
        store.transaction_provenance(noop)?
    );
    println!(
        "CONTROL unknown session writes: {}",
        store
            .provenance_transactions(0, 10, Some("unknown"), None)?
            .len()
    );
    assert_eq!(
        store
            .provenance_transactions(0, 10, Some("demo-session"), None)?
            .len(),
        1
    );
    Ok(())
}
