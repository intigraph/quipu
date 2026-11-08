//! Private pilot strict-cut primitive. No server dataset authorization,
//! serialized cut token, historical replay or composed query surface is added.

use super::*;
use std::time::Instant;

/// One read-only participant, pinned for this callback only.
/// Its local head has meaning only with this route's store identity and epoch.
pub struct CapturedRead<'a> {
    /// Immutable route, including graph, store, epoch, policy and generation.
    pub route: &'a Route,
    /// Catalog generation shared by every selected participant.
    pub catalog_revision: u64,
    /// This participant's captured committed transaction head.
    pub local_tx: i64,
    /// Read-only connection in the snapshot that produced the coordinate.
    pub store: &'a Store,
}

impl Owner {
    /// Capture a strict cut of an already authorized, bounded owner selection.
    /// Refuses immediately if any selected fence/reader is occupied; no queue,
    /// writer fallback, partial answer or weaker vector cut. The caller owns
    /// dataset authorization and evaluation budgets. There is no server route.
    pub fn read_strict<T>(
        owners: &[&Self],
        deadline: Instant,
        query: impl FnOnce(&[CapturedRead<'_>]) -> Result<T>,
    ) -> Result<T> {
        let check_deadline = || {
            if Instant::now() >= deadline {
                Err(refused("strict capture deadline expired"))
            } else {
                Ok(())
            }
        };
        check_deadline()?;
        let first = owners
            .first()
            .ok_or_else(|| refused("empty strict owner selection"))?;
        if owners.len() > first.catalog.budget().max_owners {
            return Err(refused("strict owner selection exceeds reserved fan-out"));
        }
        if owners
            .iter()
            .any(|owner| !Arc::ptr_eq(&first.catalog, &owner.catalog))
        {
            return Err(refused(
                "strict selection crosses pinned catalog generations",
            ));
        }
        let mut selected = owners.to_vec();
        selected.sort_unstable_by(|a, b| a.route.graph.cmp(&b.route.graph));
        if selected
            .windows(2)
            .any(|pair| pair[0].route.graph == pair[1].route.graph)
        {
            return Err(refused("duplicate strict owner participant"));
        }

        // Never wait in a participant's FIFO queue while retaining another
        // participant's fence. Every early return drops the partial set.
        let mut writers = Vec::with_capacity(selected.len());
        for owner in &selected {
            check_deadline()?;
            writers.push(
                owner
                    .writer
                    .try_lock()
                    .ok_or_else(|| refused("strict participant commit fence occupied"))?,
            );
            owner.placement.verify()?;
        }
        let mut readers = Vec::with_capacity(selected.len());
        for owner in &selected {
            check_deadline()?;
            let start = owner.next_reader.fetch_add(1, Ordering::Relaxed) % owner.readers.len();
            let reader = (0..owner.readers.len())
                .find_map(|offset| owner.readers[(start + offset) % owner.readers.len()].try_lock())
                .ok_or_else(|| refused("strict participant read pool occupied"))?;
            readers.push(reader);
        }

        // All selected commits are fenced before the first SQLite snapshot.
        // Identity reads establish each deferred transaction's snapshot while
        // that common barrier is held. RAII rolls every snapshot back on any
        // later identity, deadline, head-read or callback failure.
        let mut snapshots = Vec::with_capacity(selected.len());
        let mut captured = Vec::with_capacity(selected.len());
        for (owner, reader) in selected.iter().zip(&readers) {
            check_deadline()?;
            snapshots.push(reader.conn.unchecked_transaction()?);
            Self::check_identity(&owner.route, reader)?;
            captured.push(CapturedRead {
                route: &owner.route,
                catalog_revision: first.catalog.revision(),
                local_tx: reader.latest_tx_id()?,
                store: reader,
            });
        }
        check_deadline()?;
        // The last captured head is a common published instant for this
        // cooperating-owner prototype. Evaluation uses retained WAL snapshots,
        // never a current read or a vector-cut fallback after contention.
        drop(writers);
        let result = query(&captured)?;
        for snapshot in snapshots {
            snapshot.commit()?;
        }
        Ok(result)
    }
}
