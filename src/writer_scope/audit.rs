//! Server-owned refusal evidence waits for the outer data transaction to
//! resolve. This is not a capability to write caller facts outside its graph.
use crate::{Store, store::events::PendingRefusal};

#[derive(Default)]
pub(crate) struct Audit {
    verdicts: Option<(String, Option<String>)>,
    refusals: Vec<Refusal>,
}

struct Refusal {
    refusal: PendingRefusal,
    graph: String,
    actor: Option<String>,
    source: Option<String>,
    timestamp: String,
}

impl Audit {
    pub(crate) fn flush(self, store: &mut Store) {
        if !store.conn.is_autocommit() {
            return;
        }
        if let Some((timestamp, actor)) = self.verdicts {
            store.flush_pending_verdicts(&timestamp, actor.as_deref());
        }
        for r in self.refusals {
            store.record_refusal(
                &r.refusal,
                &r.graph,
                r.actor.as_deref(),
                r.source.as_deref(),
                &r.timestamp,
            );
        }
    }
}

pub(crate) fn defer_verdicts(timestamp: &str, actor: Option<&str>) -> bool {
    super::CAPABILITY.with(|slot| {
        let slot = slot.borrow();
        let Some(cap) = slot.as_ref() else {
            return false;
        };
        cap.audit.lock().expect("scope audit poisoned").verdicts =
            Some((timestamp.into(), actor.map(str::to_owned)));
        true
    })
}

pub(crate) fn defer_refusal(
    refusal: &PendingRefusal,
    graph: &str,
    actor: Option<&str>,
    source: Option<&str>,
    timestamp: &str,
) -> bool {
    super::CAPABILITY.with(|slot| {
        let slot = slot.borrow();
        let Some(cap) = slot.as_ref() else {
            return false;
        };
        cap.audit
            .lock()
            .expect("scope audit poisoned")
            .refusals
            .push(Refusal {
                refusal: refusal.clone(),
                graph: graph.into(),
                actor: actor.map(str::to_owned),
                source: source.map(str::to_owned),
                timestamp: timestamp.into(),
            });
        true
    })
}
