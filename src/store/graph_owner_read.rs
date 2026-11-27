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
    /// The head a composed reader may see in this file (Phase 3 S3): equal
    /// to `local_tx` unless an applied participant batch is not yet
    /// PUBLISHED by its home, then the pre-batch head (the old cut).
    pub published_tx: i64,
    /// Read-only connection in the snapshot that produced the coordinate.
    pub store: &'a Store,
    /// This participant is the configured legacy file (owner of ROOT), the
    /// only file an attached composition may read in term space 0.
    pub legacy: bool,
}

impl Owner {
    /// Capture a strict cut of an already authorized, bounded owner selection.
    /// Refuses immediately if any selected fence/reader is occupied; no queue,
    /// writer fallback, partial answer or weaker vector cut. The caller owns
    /// dataset authorization and evaluation budgets. A late callback result is
    /// refused; this synchronous API cannot preempt callback execution. There
    /// is no server route.
    pub fn read_strict<T>(
        owners: &[&Self],
        deadline: Instant,
        query: impl FnOnce(&[CapturedRead<'_>]) -> Result<T>,
    ) -> Result<T> {
        Self::read_strict_with(owners, deadline, |_| Ok(()), query)
    }

    /// [`Self::read_strict`], plus `under_fence`, which runs after every
    /// participant snapshot is captured and BEFORE the commit fences are
    /// released. A composition connection begins its own snapshot there, so it
    /// observes exactly the captured cut (aegis-n0wc.5 Phase 2).
    pub fn read_strict_with<T>(
        owners: &[&Self],
        deadline: Instant,
        under_fence: impl FnOnce(&[CapturedRead<'_>]) -> Result<()>,
        query: impl FnOnce(&[CapturedRead<'_>]) -> Result<T>,
    ) -> Result<T> {
        Self::read_strict_inner(owners, deadline, false, under_fence, query)
    }

    /// [`Self::read_strict_with`] when `cut` is false. With `cut` true, for a
    /// reader that honours publication cuts (the composed ATTACH reader): a
    /// file holding an applied, unpublished
    /// participant batch is captured, not refused, and its `published_tx` is
    /// the pre-batch head until the batch's home journal says PUBLISHED. A
    /// missing, foreign or undecided home refuses: the old cut is unprovable.
    /// Callers MUST bound every fact read by `published_tx`.
    pub(crate) fn read_strict_inner<T>(
        owners: &[&Self],
        deadline: Instant,
        cut: bool,
        under_fence: impl FnOnce(&[CapturedRead<'_>]) -> Result<()>,
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
        let routed_count = owners
            .iter()
            .filter(|owner| owner.route.graph != crate::schema::ROOT_GRAPH_IRI)
            .count();
        if routed_count > first.catalog.budget().max_owners {
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
                    .fence()
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
            if !cut {
                super::graph_set::participant::check_read_fence(reader)?;
            }
            let local_tx = reader.latest_tx_id()?;
            captured.push(CapturedRead {
                route: &owner.route,
                catalog_revision: first.catalog.revision(),
                local_tx,
                published_tx: local_tx,
                store: reader,
                legacy: owner.route.graph == crate::schema::ROOT_GRAPH_IRI,
            });
        }
        if cut {
            // Every selected fence is held: no batch can change state now.
            // An unselected home is read at most once per batch: recovery
            // needs every participant fence, and this reader holds the
            // selected ones, so that home's decision cannot move under us.
            let mut decided = std::collections::HashMap::new();
            let heads = selected
                .iter()
                .zip(&captured)
                .map(|(owner, read)| Self::published_head(owner, read, &captured, &mut decided))
                .collect::<Result<Vec<_>>>()?;
            for (read, head) in captured.iter_mut().zip(heads) {
                read.published_tx = head;
            }
        }
        check_deadline()?;
        under_fence(&captured)?;
        check_deadline()?;
        // The last captured head is a common published instant for this
        // cooperating-owner prototype. Evaluation uses retained WAL snapshots,
        // never a current read or a vector-cut fallback after contention.
        drop(writers);
        let result = query(&captured)?;
        check_deadline()?;
        for snapshot in snapshots {
            snapshot.commit()?;
        }
        check_deadline()?;
        Ok(result)
    }

    /// This file's publication cut, decided by its batch's home journal.
    fn published_head(
        owner: &Self,
        read: &CapturedRead<'_>,
        all: &[CapturedRead<'_>],
        decided: &mut std::collections::HashMap<String, Option<bool>>,
    ) -> Result<i64> {
        use super::graph_set::participant::{applied_intent, home_published};
        let Some(intent) = applied_intent(read.store)? else {
            return Ok(read.local_tx);
        };
        let captured_home = all.iter().find(|other| {
            other.route.store_id == intent.home_store_id
                && other.route.log_epoch == intent.home_log_epoch
        });
        let published = match captured_home {
            // The home is selected too: read it in its own captured snapshot.
            Some(home) => home_published(&home.store.conn, &intent)?,
            None if decided.contains_key(&intent.batch_id) => decided[&intent.batch_id],
            None => {
                let path = owner
                    .placement
                    .path
                    .parent()
                    .ok_or_else(|| refused("participant file has no directory"))?
                    .join(format!("{}.db", intent.home_file_id));
                if !path.is_file() {
                    return Err(refused(
                        "home participant unavailable; the old cut is not provable",
                    ));
                }
                let conn = rusqlite::Connection::open_with_flags(
                    &path,
                    rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY
                        | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
                )?;
                let published = home_published(&conn, &intent)?;
                decided.insert(intent.batch_id.clone(), published);
                published
            }
        };
        match published {
            Some(true) => Ok(read.local_tx),
            Some(false) => Ok(intent.expected_head),
            None => Err(refused(
                "home participant has no decision for this batch; the old cut is not provable",
            )),
        }
    }
}
