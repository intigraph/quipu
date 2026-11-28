//! One transition table for the knot and routed-update completion ledgers.
//! The wire shapes differ; custody, receipt and no-replay guarantees do not.
use super::{Binding, MAX_ATTEMPTS, Store, Ticket, update};
use crate::store::Datum;
use crate::types::{Op, Value as FactValue};
use serde_json::{Value, json};

#[derive(Clone, Copy, Debug)]
enum Ledger {
    Knot,
    Update,
}
#[derive(Debug)]
enum Attempt {
    Knot(Ticket),
    Update(update::UpdateTicket),
}

impl Ledger {
    fn table(self) -> &'static str {
        match self {
            Self::Knot => "knot_attempts",
            Self::Update => "update_attempts",
        }
    }
    fn admit(self, store: &mut Store, binding: &Binding) -> crate::Result<Attempt> {
        match self {
            Self::Knot => store.admit_knot_attempt(binding.clone()).map(Attempt::Knot),
            Self::Update => match store.admit_update_attempt(binding)? {
                update::UpdateAdmission::Fresh(ticket) => Ok(Attempt::Update(ticket)),
                update::UpdateAdmission::Retry(_) => panic!("expected a fresh attempt"),
            },
        }
    }
    fn status(self, store: &Store, binding: &Binding) -> crate::Result<Value> {
        match self {
            Self::Knot => store.knot_attempt_status(binding),
            Self::Update => store.update_attempt_status(binding),
        }
    }
    // Knot's HTTP adapter checks status after duplicate admission refuses;
    // update's native API returns Retry. Neither may issue another ticket.
    fn retry(self, store: &mut Store, binding: &Binding) -> Value {
        match self {
            Self::Knot => {
                assert!(store.admit_knot_attempt(binding.clone()).is_err());
                store.knot_attempt_status(binding).unwrap()
            }
            Self::Update => match store.admit_update_attempt(binding).unwrap() {
                update::UpdateAdmission::Retry(status) => status,
                update::UpdateAdmission::Fresh(_) => panic!("retry admitted a second worker"),
            },
        }
    }
    fn seal(self, store: &mut Store, binding: &Binding) -> crate::Result<Value> {
        match self {
            Self::Knot => store.seal_knot_binding(binding),
            Self::Update => store.seal_update_binding(binding),
        }
    }
    fn commit(store: &mut Store, ticket: &Attempt, n: u64) -> crate::Result<Value> {
        let datums = vec![Datum {
            entity: store.intern(&format!("urn:conformance:effect:{n}"))?,
            attribute: store.intern("urn:conformance:applied")?,
            value: FactValue::Int(1),
            valid_from: "2026-01-01T00:00:00Z".into(),
            valid_to: None,
            op: Op::Assert,
        }];
        let now = "2026-01-01T00:00:00Z";
        match ticket {
            Attempt::Knot(ticket) => {
                let tx = store.transact_knot_to_graph(
                    &datums,
                    now,
                    None,
                    None,
                    0,
                    None,
                    Some(ticket),
                )?;
                Ok(json!({"tx_id":tx,"count":1,"conforms":true}))
            }
            Attempt::Update(ticket) => {
                let (result, witnessed) = update::with_update_ticket(ticket, || {
                    store.transact_graph_batches_tx(&[(0, datums)], now, None, None)
                });
                let txs = result?;
                assert!(witnessed, "effect bypassed the update witness");
                Ok(json!({"owner":{"local_tx":txs[0].1},"asserted":1,"retracted":0}))
            }
        }
    }
    fn finish(store: &mut Store, ticket: &Attempt, receipt: Option<&Value>) -> crate::Result<()> {
        match ticket {
            Attempt::Knot(ticket) => store.finish_knot_attempt(ticket, receipt),
            Attempt::Update(ticket) => store.finish_update_attempt(ticket, receipt),
        }
    }
}

fn identity(principal: &str) -> crate::transaction_auth::Identity {
    crate::transaction_auth::Identity {
        principal: principal.into(),
        credential_id: Some("conformance-credential".into()),
        auth_class: "named_bearer".into(),
        attestation: Default::default(),
    }
}
fn binding(n: u64) -> Binding {
    Binding::for_request(format!("{n:064x}"), &json!({"intent":"one effect"})).unwrap()
}
fn effects(store: &Store) -> i64 {
    store
        .conn
        .query_row("SELECT COUNT(*) FROM facts", [], |r| r.get(0))
        .unwrap()
}

#[derive(Clone, Copy, Debug)]
enum Case {
    FinishedRetry,
    CrashSealNew,
    CommittedCannotSeal,
    ForeignBinding,
    StoppedCannotSeal,
    Capacity,
}
const CASES: &[Case] = &[
    Case::FinishedRetry,
    Case::CrashSealNew,
    Case::CommittedCannotSeal,
    Case::ForeignBinding,
    Case::StoppedCannotSeal,
    Case::Capacity,
];

#[test]
fn both_completion_ledgers_follow_one_transition_table() {
    for ledger in [Ledger::Knot, Ledger::Update] {
        for case in CASES {
            crate::transaction_auth::with_identity(Some(identity("urn:conformance:owner")), || {
                let dir = tempfile::tempdir().unwrap();
                let path = dir.path().join("ledger.db");
                let mut store = Store::open(path.to_str().unwrap()).unwrap();
                let original = binding(1);
                let ticket = ledger.admit(&mut store, &original).unwrap();
                assert_eq!(
                    ledger.status(&store, &original).unwrap()["state"],
                    "admitted"
                );
                match case {
                    Case::FinishedRetry => {
                        let receipt = Ledger::commit(&mut store, &ticket, 1).unwrap();
                        assert_eq!(
                            ledger.status(&store, &original).unwrap()["state"],
                            "committed"
                        );
                        Ledger::finish(&mut store, &ticket, Some(&receipt)).unwrap();
                        let before = effects(&store);
                        drop(store);
                        let mut store = Store::open(path.to_str().unwrap()).unwrap();
                        let retry = ledger.retry(&mut store, &original);
                        assert_eq!(retry["state"], "finished");
                        assert_eq!(retry["receipt"], receipt);
                        assert_eq!(effects(&store), before);
                        assert_eq!(before, 1);
                        assert!(ledger.seal(&mut store, &original).is_err());
                    }
                    Case::CrashSealNew => {
                        drop(store);
                        let mut store = Store::open(path.to_str().unwrap()).unwrap();
                        let retry = ledger.retry(&mut store, &original);
                        assert_eq!(retry["state"], "admitted", "{ledger:?}: UNKNOWN custody");
                        assert!(retry.get("receipt").is_none());
                        assert!(retry.get("commit").is_none());
                        assert_eq!(effects(&store), 0);
                        assert_eq!(
                            ledger.seal(&mut store, &original).unwrap()["state"],
                            "sealed"
                        );
                        assert!(ledger.seal(&mut store, &original).is_err());
                        assert!(Ledger::commit(&mut store, &ticket, 1).is_err());
                        assert_eq!(effects(&store), 0, "sealed worker's effect must roll back");
                        let fresh = binding(2);
                        let ticket = ledger.admit(&mut store, &fresh).unwrap();
                        let receipt = Ledger::commit(&mut store, &ticket, 2).unwrap();
                        Ledger::finish(&mut store, &ticket, Some(&receipt)).unwrap();
                        assert_eq!(ledger.retry(&mut store, &fresh)["receipt"], receipt);
                        assert_eq!(effects(&store), 1, "new ID applies exactly once");
                    }
                    Case::CommittedCannotSeal => {
                        Ledger::commit(&mut store, &ticket, 1).unwrap();
                        assert!(ledger.seal(&mut store, &original).is_err());
                        let retry = ledger.retry(&mut store, &original);
                        assert_eq!(retry["state"], "committed");
                        assert!(retry.get("receipt").is_none());
                        assert!(retry.get("commit").is_some());
                        assert_eq!(effects(&store), 1);
                    }
                    Case::ForeignBinding => {
                        let receipt = Ledger::commit(&mut store, &ticket, 1).unwrap();
                        Ledger::finish(&mut store, &ticket, Some(&receipt)).unwrap();
                        let mut changed = original.clone();
                        changed.body_sha256 = "b".repeat(64);
                        assert!(ledger.admit(&mut store, &changed).is_err());
                        let status = ledger.status(&store, &changed).unwrap();
                        assert_eq!(status["state"], "unknown");
                        assert!(status.get("receipt").is_none());
                        crate::transaction_auth::with_identity(
                            Some(identity("urn:conformance:other")),
                            || {
                                let foreign = binding(1);
                                assert!(ledger.admit(&mut store, &foreign).is_err());
                                let status = ledger.status(&store, &foreign).unwrap();
                                assert_eq!(status["state"], "unknown");
                                assert!(status.get("receipt").is_none());
                                assert!(status.get("commit").is_none());
                            },
                        );
                        assert_eq!(ledger.retry(&mut store, &original)["receipt"], receipt);
                        assert_eq!(effects(&store), 1);
                    }
                    Case::StoppedCannotSeal => {
                        Ledger::finish(&mut store, &ticket, None).unwrap();
                        assert_eq!(ledger.retry(&mut store, &original)["state"], "stopped");
                        assert!(ledger.seal(&mut store, &original).is_err());
                        assert_eq!(effects(&store), 0);
                    }
                    Case::Capacity => {
                        // A bounded native-ledger fixture, not 65k admitted writes.
                        let table = ledger.table();
                        store.conn.execute_batch(&format!(
                            "WITH RECURSIVE n(i) AS (SELECT 2 UNION ALL SELECT i+1 FROM n WHERE i<{MAX_ATTEMPTS}) \
                             INSERT INTO {table}(write_id,owner,body,store_id,epoch,nonce,phase) \
                             SELECT printf('%064x',i),owner,body,store_id,epoch,printf('%064x',i),'admitted' \
                             FROM n CROSS JOIN {table} WHERE write_id='{:064x}'", 1
                        )).unwrap();
                        let before: i64 = store
                            .conn
                            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
                            .unwrap();
                        assert_eq!(before, MAX_ATTEMPTS);
                        let error = ledger.admit(&mut store, &binding(1_000_000)).unwrap_err();
                        assert!(error.to_string().contains("admission capacity reached"));
                        let after: i64 = store
                            .conn
                            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
                            .unwrap();
                        assert_eq!(after, before);
                        assert_eq!(effects(&store), 0);
                    }
                }
            });
        }
    }
}
