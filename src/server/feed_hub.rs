//! The change feed's shared reader (docs/design/event-push.md §2).
//!
//! Before the hub, every `/changes/stream` connection woke on every commit,
//! crossed the writer barrier and read its own page: N followers cost N store
//! reads per transaction. The hub reads each new transaction ONCE, unfiltered,
//! into a bounded in-memory ring, and every subscriber builds its own page from
//! the ring in memory: its graph filter, its own `since`, the records renumbered
//! exactly as a filtered store read numbers them. A subscriber does no store
//! read while it is served from the ring, and an idle one costs nothing.
//!
//! The ring covers every transaction in `(floor, head]`. A subscriber behind
//! the floor (a new one resuming from an old cursor, or one that fell behind a
//! trim) is answered `CatchUp` and pages the store itself, exactly as before the
//! hub, until it reaches the ring. When the store's identity or log epoch
//! changes (a freeze or thaw re-mints it), the ring is emptied, so nobody is
//! served a page of the old history from memory.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use quipu::store::changes::member::MemberSpec;
use serde_json::{Value, json};
use tokio::sync::watch;

use crate::{SharedStore, StoreHandle, base::blocking};

/// Transactions read per store page.
const PAGE_TXS: usize = 64;
/// Bytes of records the ring holds before it trims its oldest transactions.
const RING_BYTES: usize = 64 * 1024 * 1024;
/// Distinct `member` conditions the hub evaluates per commit (aegis-n0wc.3).
pub(crate) const MAX_MEMBER_CONDITIONS: usize = 64;

/// The `(graph, entity)` labels, as change records carry them, whose subject
/// satisfies one `member` condition in that graph before or after a
/// transaction.
type Members = HashSet<(String, String)>;

/// One transaction as the hub read it: every record, in every graph.
struct Entry {
    tx: i64,
    records: Arc<Vec<Value>>,
    watermark_tx: i64,
    watermark_timestamp: Option<String>,
    bytes: usize,
    /// Membership per `member` condition live when this entry was read. A
    /// condition registered later is absent: its subscribers catch up from
    /// the store over this entry, which computes the same membership.
    member: HashMap<MemberSpec, Members>,
}

struct Ring {
    /// False until the reader's first read: every subscriber catches up.
    ready: bool,
    floor: i64,
    head: i64,
    source: Value,
    entries: VecDeque<Entry>,
    bytes: usize,
}

/// What the hub can do for a subscriber at its cursor.
pub(crate) enum Serve {
    /// The next page, built from the ring.
    Page(Value),
    /// Nothing in scope after the cursor yet; wait for the head to move.
    Wait,
    /// The cursor is behind the ring: page the store directly.
    CatchUp,
}

pub(crate) struct Hub {
    ring: Mutex<Ring>,
    head: watch::Sender<i64>,
    /// Change pages read by the hub, for the one-read-per-transaction control.
    pub(crate) reads: AtomicU64,
    /// Store pages read by attached subscribers catching up on their own.
    pub(crate) catchup_reads: AtomicU64,
    /// Membership reads made by the hub, for the per-condition cost control.
    pub(crate) member_reads: AtomicU64,
    /// Live `member` conditions and how many subscriptions hold each.
    members: Mutex<HashMap<MemberSpec, usize>>,
}

/// One subscription's hold on a `member` condition: the hub evaluates the
/// condition on every commit while any lease on it lives, and stops when the
/// last is dropped (the subscription's cursor owns it, so an ended or aborted
/// subscription releases it).
pub(crate) struct MemberLease {
    hub: Arc<Hub>,
    pub(crate) spec: MemberSpec,
}

impl Drop for MemberLease {
    fn drop(&mut self) {
        if let Ok(mut members) = self.hub.members.lock()
            && let Some(count) = members.get_mut(&self.spec)
        {
            *count -= 1;
            if *count == 0 {
                members.remove(&self.spec);
            }
        }
    }
}

impl Hub {
    /// The store's hub, started on first use.
    pub(crate) fn of(store: &SharedStore) -> Arc<Self> {
        store
            .changes_hub
            .get_or_init(|| {
                let hub = Arc::new(Self {
                    ring: Mutex::new(Ring {
                        ready: false,
                        floor: 0,
                        head: 0,
                        source: Value::Null,
                        entries: VecDeque::new(),
                        bytes: 0,
                    }),
                    head: watch::channel(0).0,
                    reads: AtomicU64::new(0),
                    catchup_reads: AtomicU64::new(0),
                    member_reads: AtomicU64::new(0),
                    members: Mutex::new(HashMap::new()),
                });
                tokio::spawn(read_loop(Arc::downgrade(store), hub.clone()));
                hub
            })
            .clone()
    }

    /// Hold `spec` for one subscription, or refuse when the hub already
    /// evaluates [`MAX_MEMBER_CONDITIONS`] other conditions.
    pub(crate) fn lease_member(self: &Arc<Self>, spec: MemberSpec) -> Result<MemberLease, String> {
        let mut members = self
            .members
            .lock()
            .map_err(|_| "member conditions unavailable".to_string())?;
        let full = members.len() >= MAX_MEMBER_CONDITIONS;
        match members.get_mut(&spec) {
            Some(count) => *count += 1,
            None if full => {
                return Err(format!(
                    "member condition capacity exhausted: the server evaluates at most \
                     {MAX_MEMBER_CONDITIONS} distinct member conditions; retry later"
                ));
            }
            None => {
                members.insert(spec.clone(), 1);
            }
        }
        Ok(MemberLease {
            hub: self.clone(),
            spec,
        })
    }

    /// The live `member` conditions.
    fn member_specs(&self) -> Vec<MemberSpec> {
        self.members
            .lock()
            .map(|m| m.keys().cloned().collect())
            .unwrap_or_default()
    }

    /// A receiver that changes whenever the ring's head moves.
    pub(crate) fn watch(&self) -> watch::Receiver<i64> {
        self.head.subscribe()
    }

    /// The next page for a subscriber at `cursor` with an optional graph IRI.
    /// `scanned` is how far this subscriber has already searched the ring for
    /// its graph, so transactions of other graphs never push it into catch-up.
    #[cfg(test)]
    pub(crate) fn serve(&self, cursor: i64, graph: Option<&str>, scanned: &mut i64) -> Serve {
        self.serve_filtered(
            cursor,
            &RecordFilter {
                graph,
                ..RecordFilter::default()
            },
            scanned,
        )
    }

    /// [`Hub::serve`] with a full [`RecordFilter`] (aegis-n0wc.3).
    pub(crate) fn serve_filtered(
        &self,
        cursor: i64,
        filter: &RecordFilter<'_>,
        scanned: &mut i64,
    ) -> Serve {
        let Ok(ring) = self.ring.lock() else {
            return Serve::CatchUp;
        };
        let from = cursor.max(*scanned);
        if !ring.ready || from < ring.floor {
            return Serve::CatchUp;
        }
        for entry in ring.entries.iter().filter(|e| e.tx > from) {
            let records: Vec<Value> = if filter.is_unfiltered() {
                entry.records.as_ref().clone()
            } else {
                let mut kept: Vec<&Value> =
                    entry.records.iter().filter(|r| filter.matches(r)).collect();
                if let Some(spec) = filter.member
                    && !kept.is_empty()
                {
                    // Membership is evaluated in each record's own graph:
                    // with `graph` set, every kept record is in it.
                    let Some(members) = entry.member.get(spec) else {
                        return Serve::CatchUp;
                    };
                    kept.retain(|r| {
                        let label = |key: &str| r[key].as_str().unwrap_or_default().to_string();
                        members.contains(&(label("graph"), label("entity")))
                    });
                }
                kept.into_iter()
                    .enumerate()
                    .map(|(sequence, r)| {
                        let mut r = r.clone();
                        r["sequence"] = json!(sequence);
                        r
                    })
                    .collect()
            };
            if records.is_empty() {
                continue;
            }
            return Serve::Page(json!({
                "records": records,
                "since": cursor,
                "next_tx": entry.tx,
                "watermark_tx": entry.watermark_tx,
                "watermark_timestamp": entry.watermark_timestamp,
                "capture": "old_and_new_values",
                "source": ring.source,
            }));
        }
        *scanned = ring.head;
        Serve::Wait
    }
}

/// Which records of a transaction one change subscriber receives: all of
/// graph, predicates, subjects and membership that are set must match
/// (aegis-n0wc.3). The store's catch-up page applies the same filter (graph,
/// predicates and subjects in SQL, membership with the same store helper the
/// hub uses), so a ring page and a store page stay equal.
#[derive(Debug, Default)]
pub(crate) struct RecordFilter<'a> {
    pub(crate) graph: Option<&'a str>,
    pub(crate) predicates: Option<HashSet<&'a str>>,
    pub(crate) subjects: Option<HashSet<&'a str>>,
    /// A `member` condition the subscription holds a [`MemberLease`] on.
    pub(crate) member: Option<&'a MemberSpec>,
}

impl RecordFilter<'_> {
    fn is_unfiltered(&self) -> bool {
        self.graph.is_none()
            && self.predicates.is_none()
            && self.subjects.is_none()
            && self.member.is_none()
    }

    fn matches(&self, record: &Value) -> bool {
        let field_in = |key: &str, set: &Option<HashSet<&str>>| {
            set.as_ref()
                .is_none_or(|set| record[key].as_str().is_some_and(|v| set.contains(v)))
        };
        self.graph
            .is_none_or(|iri| record["graph"].as_str() == Some(iri))
            && field_in("attribute", &self.predicates)
            && field_in("entity", &self.subjects)
    }
}

/// The hub's one reader: read every new transaction after the head, then
/// wait for the next commit. Ends when the store is gone.
async fn read_loop(store: Weak<StoreHandle>, hub: Arc<Hub>) {
    let Some(mut hints) = store.upgrade().map(|s| s.commit_wake.subscribe()) else {
        return;
    };
    hints.borrow_and_update();
    loop {
        let Some(shared) = store.upgrade() else {
            return;
        };
        let reader = hub.clone();
        let read = blocking(move || {
            // Let the hinted commit finish before reading it.
            drop(shared.writer.lock());
            read_new(&shared, &reader);
            Ok(())
        })
        .await;
        if read.is_err() {
            // Unreadable: empty the ring so subscribers page the store, which
            // reports the failure to each of them.
            if let Ok(mut ring) = hub.ring.lock() {
                ring.ready = false;
                ring.entries.clear();
                ring.bytes = 0;
            }
        }
        if tokio::time::timeout(Duration::from_secs(3600), hints.changed())
            .await
            .is_ok_and(|r| r.is_err())
        {
            return;
        }
    }
}

/// Read every transaction after the ring's head into the ring.
fn read_new(store: &StoreHandle, hub: &Hub) {
    let read = store.read();
    let Ok(source) = read.store_id().and_then(|id| Ok((id, read.log_epoch()?))) else {
        return;
    };
    let source = json!({"store_id": source.0, "log_epoch": source.1});
    let mut moved = false;
    {
        let Ok(mut ring) = hub.ring.lock() else {
            return;
        };
        if !ring.ready || ring.source != source {
            // First read, or a different history: start empty at the head.
            let Ok(head) = read.latest_tx_id() else {
                return;
            };
            ring.entries.clear();
            ring.bytes = 0;
            ring.floor = head;
            ring.head = head;
            ring.source = source.clone();
            ring.ready = true;
            moved = true;
        }
    }
    // A wake with nothing new (a hint for a commit already read, or one that
    // wrote no facts) costs one id lookup, never a page read.
    let Ok(latest) = read.latest_tx_id() else {
        return;
    };
    // With no `member` subscriber this is empty and costs no read.
    let specs = hub.member_specs();
    loop {
        let head = match hub.ring.lock() {
            Ok(ring) => ring.head,
            Err(_) => return,
        };
        if latest <= head {
            break;
        }
        hub.reads.fetch_add(1, Ordering::Relaxed);
        let Ok(page) = read.changes_after(
            head,
            PAGE_TXS,
            quipu::store::changes::Capture::OldAndNewValues,
            None,
        ) else {
            return;
        };
        if page.records.is_empty() {
            break;
        }
        // A short page reached the head: no second read to learn that.
        let short = page.next_tx == page.watermark_tx;
        let mut by_tx: std::collections::BTreeMap<i64, Vec<Value>> = Default::default();
        for record in page.records {
            by_tx
                .entry(record["tx"].as_i64().unwrap_or_default())
                .or_default()
                .push(record);
        }
        let short = short || by_tx.len() < PAGE_TXS;
        let members: Vec<HashMap<MemberSpec, Members>> = by_tx
            .iter()
            .map(|(tx, records)| members_of(&read, hub, &specs, *tx, records))
            .collect();
        let Ok(mut ring) = hub.ring.lock() else {
            return;
        };
        for ((tx, records), member) in by_tx.into_iter().zip(members) {
            let bytes = records.iter().map(|r| r.to_string().len()).sum::<usize>()
                + member
                    .values()
                    .flatten()
                    .map(|(g, e)| g.len() + e.len())
                    .sum::<usize>();
            ring.entries.push_back(Entry {
                tx,
                records: Arc::new(records),
                watermark_tx: page.watermark_tx,
                watermark_timestamp: page.watermark_timestamp.clone(),
                bytes,
                member,
            });
            ring.bytes += bytes;
            ring.head = tx;
        }
        while ring.bytes > RING_BYTES {
            let Some(old) = ring.entries.pop_front() else {
                break;
            };
            ring.bytes -= old.bytes;
            ring.floor = old.tx;
        }
        moved = true;
        if short {
            break;
        }
    }
    if moved && let Ok(ring) = hub.ring.lock() {
        hub.head.send_replace(ring.head);
    }
}

/// Membership of one transaction's subjects for each live condition: one
/// batched read per condition (per 500 subjects). A condition whose read
/// fails is left out, so its subscribers catch up from the store over this
/// transaction and hear the failure there; a condition naming a term the
/// store has never seen holds for nobody.
fn members_of(
    read: &quipu::Store,
    hub: &Hub,
    specs: &[MemberSpec],
    tx: i64,
    records: &[Value],
) -> HashMap<MemberSpec, Members> {
    let mut out = HashMap::new();
    if specs.is_empty() {
        return out;
    }
    let mut labels: HashMap<(i64, i64), (String, String)> = HashMap::new();
    for record in records {
        match read.record_pair(record) {
            Ok(Some(pair)) => {
                let label = |key: &str| record[key].as_str().unwrap_or_default().to_string();
                labels.insert(pair, (label("graph"), label("entity")));
            }
            Ok(None) => {}
            // Unresolvable: no condition is recorded, so all catch up.
            Err(_) => return out,
        }
    }
    let pairs: Vec<(i64, i64)> = labels.keys().copied().collect();
    for spec in specs {
        let found = match read.member_condition(spec) {
            Ok(None) => Ok(HashSet::new()),
            Ok(Some(condition)) => read.member_pairs(tx, &condition, &pairs).map(|(found, n)| {
                hub.member_reads.fetch_add(n, Ordering::Relaxed);
                found
            }),
            Err(e) => Err(e),
        };
        if let Ok(found) = found {
            let members = found
                .iter()
                .filter_map(|p| labels.get(p).cloned())
                .collect();
            out.insert(spec.clone(), members);
        }
    }
    out
}

#[cfg(test)]
#[path = "feed_hub_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "feed_member_tests.rs"]
mod member_tests;
