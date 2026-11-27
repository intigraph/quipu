//! One shared label snapshot, maintained only from complete committed changes.
use super::LabelIndex;
use crate::{
    error::Result,
    namespace,
    store::{Datum, Store},
};
use parking_lot::Mutex;
use std::{collections::HashSet, sync::Arc};

pub(crate) type SharedCache = Arc<Mutex<Option<CachedLabels>>>;
#[derive(Clone, PartialEq, Eq)]
struct Key {
    store: String,
    path: Option<String>,
    tx: i64,
    /// The change-log epoch: a freeze deletes rows without leaving a delta,
    /// and mints a new epoch, so a snapshot never catches up across one.
    epoch: Option<String>,
    attachments: Vec<crate::store::attach::Attachment>,
}
impl Key {
    /// Same store generation, only further along: what a delta can bridge.
    fn behind(&self, now: &Self) -> bool {
        self.tx < now.tx
            && self.store == now.store
            && self.path == now.path
            && self.epoch.is_some()
            && self.epoch == now.epoch
            && self.attachments == now.attachments
    }
    fn of(store: &Store) -> Result<Self> {
        Ok(Self {
            store: store.store_id()?,
            path: store.conn.path().map(|p| {
                std::fs::canonicalize(p)
                    .map_or_else(|_| p.to_string(), |p| p.to_string_lossy().into_owned())
            }),
            tx: store.latest_tx_id()?,
            epoch: store.log_epoch().ok(),
            attachments: store.attachments.clone(),
        })
    }
}
pub(crate) struct CachedLabels {
    key: Key,
    index: Arc<LabelIndex>,
}

/// Never trust a process-local invalidation signal instead of the database.
/// Read-only attachments do not certify immutable provider lifetimes, so composed
/// stores always use exact scans instead of caching an uncertified generation.
/// Cold/stale requests use the original exact scan until the off-request warmer
/// publishes a current snapshot. They never wait for or serve a stale snapshot.
pub(super) fn get(store: &Store) -> Result<Arc<LabelIndex>> {
    if !store.resolution_config.cache_labels || !store.attachments.is_empty() {
        *store.resolution_cache.lock() = None;
    } else if store.conn.is_autocommit() {
        let Ok(key) = Key::of(store) else {
            return Ok(Arc::new(LabelIndex::scan(store)?));
        };
        let resident = store
            .resolution_cache
            .lock()
            .as_ref()
            .map(|c| (c.key.clone(), c.index.clone()));
        if let Some((cached, index)) = resident {
            if cached == key {
                return Ok(index);
            }
            if let Some(index) = advance(store, &cached, &index, &key)? {
                return Ok(index);
            }
        }
    }
    Ok(Arc::new(LabelIndex::scan(store)?))
}

/// Bring a stale snapshot of the same generation current from the delta and
/// publish it, unless a newer generation was published meanwhile. `None` when
/// the delta cannot certify it (see [`super::cache_delta`]).
fn advance(
    store: &Store,
    cached: &Key,
    index: &Arc<LabelIndex>,
    key: &Key,
) -> Result<Option<Arc<LabelIndex>>> {
    if !cached.behind(key) {
        return Ok(None);
    }
    let Some(index) = super::cache_delta::catch_up(store, index, cached.tx, key.tx)? else {
        return Ok(None);
    };
    let mut cache = store.resolution_cache.lock();
    if cache
        .as_ref()
        .is_none_or(|c| c.key.store != key.store || c.key.tx <= key.tx)
    {
        *cache = Some(CachedLabels {
            key: key.clone(),
            index: index.clone(),
        });
    }
    Ok(Some(index))
}

impl Store {
    /// Warm resolution labels before accepting traffic or from a commit-hint
    /// worker. Returns false if a concurrent write prevents a coherent publish.
    /// A disabled cache or a composed store uses the original per-request scan and
    /// retains no index; read-only mounting does not guarantee provider immutability.
    ///
    /// # Errors
    /// Propagates label-scan/store errors; no write is performed.
    pub fn prepare_resolution_index(&self) -> Result<bool> {
        if !self.resolution_config.cache_labels || !self.attachments.is_empty() {
            *self.resolution_cache.lock() = None;
            return Ok(false);
        }
        if !self.conn.is_autocommit() {
            return Ok(false);
        }
        let key = Key::of(self)?;
        let resident = self
            .resolution_cache
            .lock()
            .as_ref()
            .map(|c| (c.key.clone(), c.index.clone()));
        if let Some((cached, index)) = resident
            && (cached == key || advance(self, &cached, &index, &key)?.is_some())
        {
            return Ok(true);
        }
        let index = Arc::new(LabelIndex::scan(self)?);
        // A write between generation and scan must not stamp old labels with a
        // new transaction id. A failed publication simply leaves the scan path.
        if Key::of(self)? != key {
            return Ok(false);
        }
        *self.resolution_cache.lock() = Some(CachedLabels { key, index });
        Ok(true)
    }
}

/// Called after RELEASE and after nested commit hooks. Unknown effects, aliases,
/// topology changes and outer savepoints invalidate instead of guessing.
pub(crate) fn maintain(store: &Store, effective: Option<&[Datum]>, tx: i64) {
    if let Err(error) = maintain_checked(store, effective, tx) {
        *store.resolution_cache.lock() = None;
        eprintln!("resolution label cache invalidated: {error}");
    }
}
fn maintain_checked(store: &Store, effective: Option<&[Datum]>, tx: i64) -> Result<()> {
    let key = Key::of(store)?;
    let cached = {
        let mut cache = store.resolution_cache.lock();
        if cache.as_ref().is_some_and(|c| c.key == key) {
            return Ok(());
        }
        let Some(cached) = cache.take() else {
            return Ok(());
        };
        cached
    };
    let Some(effective) = effective else {
        return Ok(());
    };
    if !store.resolution_config.cache_labels || !store.conn.is_autocommit() {
        return Ok(());
    }
    let before: i64 = store.conn.query_row(
        "SELECT COALESCE(MAX(id),0) FROM transactions WHERE id < ?1",
        [tx],
        |r| r.get(0),
    )?;
    if key.tx != tx
        || cached.key.tx != before
        || cached.key.store != key.store
        || cached.key.path != key.path
        || cached.key.attachments != key.attachments
        || cached.key.epoch != key.epoch
    {
        return Ok(());
    }
    let same_as = store.lookup_all(&format!("{}sameAs", namespace::OWL))?;
    if effective.iter().any(|d| same_as.contains(&d.attribute)) {
        return Ok(());
    }
    let labels = store.lookup_all(&format!("{}label", namespace::RDFS))?;
    let affected: HashSet<i64> = effective
        .iter()
        .filter(|d| labels.contains(&d.attribute))
        .map(|d| store.canonical_id(d.entity))
        .collect::<Result<_>>()?;
    let index = super::cache_delta::rebuild(store, &cached.index, &affected, &labels)?;
    // Another warmer may have published meanwhile. Never replace a newer
    // generation with an older one; all readers also verify the database stamp.
    let mut cache = store.resolution_cache.lock();
    if cache
        .as_ref()
        .is_none_or(|c| c.key.store != key.store || c.key.tx <= key.tx)
    {
        *cache = Some(CachedLabels { key, index });
    }
    Ok(())
}

#[cfg(test)]
pub(super) fn resident_generation(store: &Store) -> Option<i64> {
    store.resolution_cache.lock().as_ref().map(|c| c.key.tx)
}
