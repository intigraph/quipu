//! Admission receipts survive term-space rewriting byte for byte.
use super::*;

fn raw_receipt(store: &Store) -> (String, String, String, i64) {
    store
        .conn
        .query_row(
            "SELECT operation_id, fingerprint, receipt, tx FROM client_changeset_receipts",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .unwrap()
}

#[test]
fn accepted_receipt_survives_respace_but_old_epoch_retry_is_refused() {
    let mut f = fixture();
    let p = proposal(&f);
    let accepted = f.store.admit_client_changeset(&p).unwrap();
    assert!(matches!(
        &accepted,
        Admission::Accepted {
            replayed: false,
            ..
        }
    ));
    let before = raw_receipt(&f.store);
    let tx = f.store.latest_tx_id().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source.db");
    let destination = dir.path().join("respaced.db");
    f.store
        .conn
        .execute("VACUUM INTO ?1", [source.to_str().unwrap()])
        .unwrap();
    crate::store::respace::respace_file(&source, &destination, 7).unwrap();
    let mut reopened = Store::open(destination.to_str().unwrap()).unwrap();
    assert_eq!(raw_receipt(&reopened), before);
    assert_eq!(reopened.latest_tx_id().unwrap(), tx);
    let Admission::Accepted { receipt, .. } = accepted else {
        panic!("expected accepted receipt");
    };
    assert_eq!(
        reopened
            .client_changeset_receipt(&before.0)
            .unwrap()
            .as_ref(),
        Some(receipt.as_ref())
    );
    assert!(
        reopened
            .admit_client_changeset(&p)
            .unwrap_err()
            .to_string()
            .contains("foreign-epoch")
    );
    assert_eq!(raw_receipt(&reopened), before);
    assert_eq!(reopened.latest_tx_id().unwrap(), tx);
}
