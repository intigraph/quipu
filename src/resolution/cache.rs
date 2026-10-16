//! One shared label snapshot, maintained only from complete committed changes.
use super::{
    LabelIndex,
    index::{LabelEntry, LabelProfile},
};
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
    attachments: Vec<crate::store::attach::Attachment>,
}
impl Key {
    fn of(store: &Store) -> Result<Self> {
        Ok(Self {
            store: store.store_id()?,
            path: store.conn.path().map(|p| {
                std::fs::canonicalize(p)
                    .map_or_else(|_| p.to_string(), |p| p.to_string_lossy().into_owned())
            }),
            tx: store.latest_tx_id()?,
            attachments: store.attachments.clone(),
        })
    }
}
pub(crate) struct CachedLabels {
    key: Key,
    index: Arc<LabelIndex>,
}

/// Never trust a process-local invalidation signal instead of the database.
/// Cold/stale requests use the original exact scan until the off-request warmer
/// publishes a current snapshot. They never wait for or serve a stale snapshot.
pub(super) fn get(store: &Store) -> Result<Arc<LabelIndex>> {
    if !store.resolution_config.cache_labels {
        *store.resolution_cache.lock() = None;
    } else if store.conn.is_autocommit() {
        let Ok(key) = Key::of(store) else {
            return Ok(Arc::new(LabelIndex::scan(store)?));
        };
        if let Some(cached) = store.resolution_cache.lock().as_ref()
            && cached.key == key
        {
            return Ok(cached.index.clone());
        }
    }
    Ok(Arc::new(LabelIndex::scan(store)?))
}

impl Store {
    /// Warm resolution labels before accepting traffic or from a commit-hint
    /// worker. Returns false if a concurrent write prevents a coherent publish.
    /// A disabled cache uses the original per-request scan and retains no index.
    ///
    /// # Errors
    /// Propagates label-scan/store errors; no write is performed.
    pub fn prepare_resolution_index(&self) -> Result<bool> {
        if !self.resolution_config.cache_labels {
            *self.resolution_cache.lock() = None;
            return Ok(false);
        }
        if !self.conn.is_autocommit() {
            return Ok(false);
        }
        let key = Key::of(self)?;
        if self
            .resolution_cache
            .lock()
            .as_ref()
            .is_some_and(|c| c.key == key)
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
    let mut index = cached.index;
    if !affected.is_empty() {
        // Read the final current rows for the affected identities, including
        // unchanged labels in attached layers and other named graphs. Applying
        // caller datums directly would lose duplicate producers and closures.
        let mut ids: Vec<i64> = affected.iter().copied().collect();
        if !store.attachments.is_empty() {
            let sql = format!(
                "SELECT alias_id FROM term_alias WHERE canonical_id IN ({})",
                super::id_list(&ids)
            );
            let mut stmt = store.conn.prepare(&sql)?;
            let aliases = stmt
                .query_map([], |r| r.get::<_, i64>(0))?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            ids.extend(aliases);
        }
        let sql = format!(
            "SELECT e,v FROM {} WHERE a IN ({}) AND e IN ({}) AND valid_to IS NULL AND op=1",
            store.facts_source(),
            super::id_list(&labels),
            super::id_list(&ids)
        );
        let mut stmt = store.conn.prepare(&sql)?;
        let mut rows = stmt.query([])?;
        let index = Arc::make_mut(&mut index);
        index
            .entries
            .retain(|entry| !affected.contains(&entry.entity_id));
        while let Some(row) = rows.next()? {
            let bytes: Vec<u8> = row.get(1)?;
            if let Ok(crate::types::Value::Str(label)) = crate::types::Value::from_bytes(&bytes) {
                let lower = label.to_lowercase();
                index.entries.push(Arc::new(LabelEntry {
                    entity_id: store.canonical_id(row.get(0)?)?,
                    profile: LabelProfile::of(&lower),
                    lower,
                }));
            }
        }
    }
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
