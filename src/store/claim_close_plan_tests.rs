use super::{CLAIM_CLOSE_SQL, Store};
use rusqlite::params;

#[test]
fn physical_claim_close_probes_owner_by_transaction_primary_key() {
    let store = Store::open_in_memory().unwrap();
    // Inspect the statement the actual writer prepares, not a copied query.
    let mut explain = store
        .conn
        .prepare(&format!("EXPLAIN QUERY PLAN {CLAIM_CLOSE_SQL}"))
        .unwrap();
    let plan = explain
        .query_map(
            params![
                "2026-01-01T00:00:00Z",
                1_i64,
                2_i64,
                crate::Value::Str("value".into()).to_bytes(),
                0_i64,
                1_i64,
                "producer",
                Option::<i64>::None,
                Option::<i64>::None,
            ],
            |row| row.get::<_, String>(3),
        )
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    assert!(
        plan.iter()
            .any(|step| step.contains("SEARCH t USING INTEGER PRIMARY KEY")),
        "owner lookup must probe a physical claim's transaction: {plan:?}"
    );
    assert!(
        !plan.iter().any(|step| step.contains("LIST SUBQUERY")),
        "each close must not rebuild a source-wide transaction list: {plan:?}"
    );
}
