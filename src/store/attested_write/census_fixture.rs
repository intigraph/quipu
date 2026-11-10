//! Fixture-only receipt/outbox experiment. No production registration or schema.
use super::*;
use crate::merge_core::census::aggregate::{AggregateSink, ProducerBinding, RecordOutcome};
use crate::merge_core::census::{
    BaseEvidence, BoundEvidence, ObservationInput, SlotKey, SlotReport,
};
use crate::merge_core::{Card, Change, SlotInput};
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};

mod cases;
mod mutants;
use mutants::{Mutation, mutation};

const LIMIT: usize = 64;
const PROFILE: &str = "fixture-only-card-one-v1";

#[derive(Clone, Copy, Default)]
enum Fault {
    #[default]
    None,
    Sql,
}

#[derive(Clone)]
struct Capture {
    nonce: String,
    store_id: String,
    epoch: String,
    base: i64,
    profile: String,
    capacity: usize,
    fault: Fault,
}

thread_local! {
    static ACTIVE: RefCell<Option<Capture>> = const { RefCell::new(None) };
    static LOST: Cell<u64> = const { Cell::new(0) };
    static EARLY_EXPORTS: Cell<u64> = const { Cell::new(0) };
}

struct Armed;
impl Drop for Armed {
    fn drop(&mut self) {
        ACTIVE.with(|p| p.replace(None));
    }
}

fn arm(store: &Store, nonce: &str, capacity: usize, fault: Fault) -> Armed {
    LOST.with(|n| n.set(0));
    EARLY_EXPORTS.with(|n| n.set(0));
    ACTIVE.with(|p| {
        assert!(p.borrow().is_none());
        p.replace(Some(Capture {
            nonce: nonce.into(),
            store_id: store.store_id().unwrap(),
            epoch: store.log_epoch().unwrap(),
            base: store.latest_tx_id().unwrap(),
            profile: PROFILE.into(),
            capacity,
            fault,
        }));
    });
    Armed
}

fn install(store: &Store) {
    let schema = if mutation() == Mutation::VolatileOnly {
        "CREATE TEMP TABLE"
    } else {
        "CREATE TABLE"
    };
    store
        .conn
        .execute_batch(&format!(
            "{schema} fixture_census_outbox (
         nonce TEXT PRIMARY KEY, store_id TEXT NOT NULL, epoch TEXT NOT NULL,
         payload TEXT NOT NULL, data_change INTEGER NOT NULL,
         exported INTEGER NOT NULL DEFAULT 0 CHECK(exported IN(0,1)));",
        ))
        .unwrap();
}

#[derive(Serialize, Deserialize)]
struct Wire {
    graph: i64,
    predicate: i64,
    change: Change,
    holds: bool,
}

fn refuse_capture() -> Error {
    refuse("fixture census coverage unavailable")
}

/// Called only by a cfg(test) tap after native receipt verification, before data.
pub(super) fn capture(store: &Store, data_change: bool, ts: &str) -> Result<()> {
    let Some(capture) = ACTIVE.with(|p| p.borrow().clone()) else {
        return Ok(());
    };
    let staged = (|| -> Result<()> {
        store.conn.execute_batch("SAVEPOINT fixture_census")?;
        let staged = stage(store, &capture, data_change, ts);
        if staged.is_err() {
            crate::store::release::rollback_release(&store.conn, "fixture_census")?;
            return staged;
        }
        crate::store::release::release(&store.conn, "fixture_census")?;
        if mutation() == Mutation::PublishInnerRelease {
            EARLY_EXPORTS.with(|n| n.set(n.get() + 1));
        }
        Ok(())
    })();
    if staged.is_err() {
        LOST.with(|n| n.set(n.get().saturating_add(1)));
        if mutation() == Mutation::PropagateObserverError {
            return staged;
        }
    }
    Ok(())
}

fn stage(store: &Store, capture: &Capture, data_change: bool, ts: &str) -> Result<()> {
    if capture.store_id != store.store_id()?
        || capture.epoch != store.log_epoch()?
        || capture.base != store.latest_tx_id()?
        || capture.profile != PROFILE
        || capture.capacity > LIMIT
        || !store.attest_verify_receipt(&capture.nonce)?
    {
        return Err(refuse_capture());
    }
    let count: i64 =
        store
            .conn
            .query_row("SELECT COUNT(*) FROM fixture_census_outbox", [], |r| {
                r.get(0)
            })?;
    if count >= i64::try_from(capture.capacity).map_err(|_| refuse_capture())? {
        return Err(refuse_capture());
    }
    let (receipt, _) = store
        .attest_receipt(&capture.nonce)?
        .ok_or_else(refuse_capture)?;
    let [datum] = receipt.challenge.proposal.datums.as_slice() else {
        return Err(refuse_capture());
    };
    let TypedValue::Str(incoming) = &datum.o else {
        return Err(refuse_capture());
    };
    let graph = store
        .registered_graph_id(&receipt.challenge.proposal.graph)?
        .ok_or_else(refuse_capture)?;
    let entity = store.lookup(&datum.s)?.ok_or_else(refuse_capture)?;
    let attribute = store.lookup(&datum.p)?.ok_or_else(refuse_capture)?;
    let facts = store.bounded_slot_facts_at(
        graph,
        entity,
        attribute,
        &crate::store::AsOf {
            tx: Some(capture.base),
            valid_at: Some(ts.into()),
        },
        crate::store::slot_reads::SlotReadLimits {
            max_claims: 64,
            max_bytes: 65536,
        },
    )?;
    let mut values = BTreeSet::new();
    for fact in facts {
        let Value::Str(value) = fact.value else {
            return Err(refuse_capture());
        };
        values.insert(value);
    }
    // This fixture owns the retained cut. It is NOT a client-declared base.
    let slot = SlotInput {
        base: values.clone(),
        ours: values.into_iter().map(|v| (v, ts.to_string())).collect(),
        theirs: BTreeMap::from([(incoming.clone(), ts.into())]),
        theirs_closed: BTreeMap::new(),
        card: Card::ONE,
    };
    let reports = crate::merge_core::census::observe_batch(
        &BTreeMap::from([(
            SlotKey {
                graph,
                entity,
                predicate: attribute,
            },
            ObservationInput {
                slot: &slot,
                base: BaseEvidence::RetainedSnapshot,
                shape_bound_known: true,
            },
        )]),
        ts,
    )?;
    let [report] = reports.as_slice() else {
        return Err(refuse_capture());
    };
    let wire = Wire {
        graph,
        predicate: attribute,
        change: report.classification.ok_or_else(refuse_capture)?,
        holds: report.would_hold_slot.ok_or_else(refuse_capture)?,
    };
    let payload = serde_json::to_string(&wire).map_err(|_| refuse_capture())?;
    if payload.len() > 4096 {
        return Err(refuse_capture());
    }
    if matches!(capture.fault, Fault::Sql) {
        store
            .conn
            .execute("INSERT INTO fixture_missing_table VALUES(1)", [])?;
    }
    store.conn.execute(
        "INSERT INTO fixture_census_outbox(nonce,store_id,epoch,payload,data_change)
         VALUES(?1,?2,?3,?4,?5)",
        params![
            capture.nonce,
            capture.store_id,
            capture.epoch,
            payload,
            data_change
        ],
    )?;
    Ok(())
}

struct Sink {
    bounded: AggregateSink,
    receipts: rusqlite::Connection,
}

impl Sink {
    fn open(path: &std::path::Path) -> Self {
        let receipts = rusqlite::Connection::open(path).unwrap();
        receipts
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS accepted(
            store_id TEXT,epoch TEXT,nonce TEXT,payload TEXT,
            PRIMARY KEY(store_id,epoch,nonce))",
            )
            .unwrap();
        Self {
            bounded: AggregateSink::default(),
            receipts,
        }
    }

    fn count(&self) -> i64 {
        self.receipts
            .query_row("SELECT COUNT(*) FROM accepted", [], |r| r.get(0))
            .unwrap()
    }

    fn accept(&self, store_id: &str, epoch: &str, nonce: &str, payload: &str) -> Result<bool> {
        let prior: Option<String> = self
            .receipts
            .query_row(
                "SELECT payload FROM accepted WHERE store_id=?1 AND epoch=?2 AND nonce=?3",
                params![store_id, epoch, nonce],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(prior) = prior {
            return Ok(prior == payload);
        }
        let wire: Wire = serde_json::from_str(payload).map_err(|_| refuse_capture())?;
        let report = SlotReport {
            graph_id: wire.graph,
            predicate_id: wire.predicate,
            base_evidence: BaseEvidence::RetainedSnapshot,
            shape_bound: BoundEvidence::Known { max_count: Some(1) },
            classification: Some(wire.change),
            would_hold_slot: Some(wire.holds),
            unknown_reason: None,
        };
        if self.bounded.try_record(
            ProducerBinding {
                source_revision: [1; 32],
                profile_revision: Some([2; 32]),
            },
            &[report],
        ) != RecordOutcome::Accepted
        {
            return Ok(false);
        }
        self.receipts.execute(
            "INSERT INTO accepted VALUES(?1,?2,?3,?4)",
            params![store_id, epoch, nonce, payload],
        )?;
        Ok(true)
    }
}

#[derive(Default, Debug, PartialEq, Eq)]
struct Scan {
    committed: usize,
    no_data: usize,
    gaps: usize,
    delivered: usize,
}

fn reconcile(store: &Store, sink: &Sink, lose_ack: bool) -> Result<Scan> {
    if !store.conn.is_autocommit() {
        return Err(refuse_capture());
    }
    let transaction = store.conn.unchecked_transaction()?;
    let scan = reconcile_snapshot(store, sink, lose_ack)?;
    transaction.commit()?;
    Ok(scan)
}

type PendingRow = (String, String, String, bool, bool);

fn reconcile_snapshot(store: &Store, sink: &Sink, lose_ack: bool) -> Result<Scan> {
    let mut query = store
        .conn
        .prepare("SELECT nonce FROM attest_receipts ORDER BY rowid LIMIT 65")?;
    let nonces = query
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    if nonces.len() > LIMIT {
        return Err(refuse_capture());
    }
    let mut nonces = nonces;
    if mutation() == Mutation::OneWakeOneWrite {
        nonces.truncate(1);
    }
    if mutation() == Mutation::TrustClientResult {
        return Ok(Scan::default());
    }
    let mut scan = Scan::default();
    if mutation() == Mutation::ReservationIsCommit && nonces.is_empty() && store.latest_tx_id()? > 0
    {
        scan.committed = 1;
    }
    for nonce in nonces {
        let evidence: Option<PendingRow> = store
            .conn
            .query_row(
                "SELECT store_id,epoch,payload,data_change,exported
             FROM fixture_census_outbox WHERE nonce=?1",
                [&nonce],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .optional()?;
        let Some((id, epoch, payload, data_change, exported)) = evidence else {
            scan.gaps += 1;
            continue;
        };
        let (_, tx) = store.attest_receipt(&nonce)?.ok_or_else(refuse_capture)?;
        if id != store.store_id()?
            || epoch != store.log_epoch()?
            || !store.attest_verify_receipt(&nonce)?
            || payload.len() > 4096
            || (data_change && !tx.is_some_and(|tx| tx > 0))
            || (!data_change && tx.is_some())
        {
            return Err(refuse_capture());
        }
        if data_change || mutation() == Mutation::ReceiptIsData {
            scan.committed += 1;
        } else {
            scan.no_data += 1;
        }
        if !exported && sink.accept(&id, &epoch, &nonce, &payload)? {
            scan.delivered += 1;
            if !lose_ack {
                store.conn.execute(
                    "UPDATE fixture_census_outbox SET exported=1 WHERE nonce=?1",
                    [&nonce],
                )?;
            }
        }
    }
    Ok(scan)
}
