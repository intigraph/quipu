//! A bounded, statement-local snapshot of the composed term aliases.

use std::{cell::RefCell, collections::HashMap};

use super::TemporalContext;
use super::pattern_util::check_eval_budget;
use crate::{Result, Store};

#[cfg(test)]
thread_local! {
    static PRECEDING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[cfg(test)]
pub(super) fn with_preceding<T>(run: impl FnOnce() -> T) -> T {
    struct Reset(bool);
    impl Drop for Reset {
        fn drop(&mut self) {
            PRECEDING.set(self.0);
        }
    }
    let _reset = Reset(PRECEDING.replace(true));
    run()
}

/// One leaf's canonical mappings, never shared across stores or statements.
pub(super) struct Canonicalizer<'a> {
    store: &'a Store,
    aliases: Option<HashMap<i64, i64>>,
    indexed_cache: Option<RefCell<HashMap<i64, i64>>>,
}

impl<'a> Canonicalizer<'a> {
    pub(super) fn new(store: &'a Store, aliases: Option<HashMap<i64, i64>>) -> Self {
        let indexed_cache =
            (store.has_attachments() && aliases.is_none()).then(|| RefCell::new(HashMap::new()));
        Self {
            store,
            aliases,
            indexed_cache,
        }
    }

    pub(super) fn resolve(&self, id: i64) -> Result<i64> {
        if let Some(aliases) = &self.aliases {
            return Ok(aliases.get(&id).copied().unwrap_or(id));
        }
        #[cfg(test)]
        if PRECEDING.get() {
            return self.store.canonical_id(id);
        }
        let Some(cache) = &self.indexed_cache else {
            return self.store.canonical_id(id);
        };
        if let Some(canonical) = cache.borrow().get(&id).copied() {
            return Ok(canonical);
        }
        let canonical = self.store.canonical_id(id)?;
        let mut cache = cache.borrow_mut();
        if cache.len() < 512 {
            cache.insert(id, canonical);
        }
        Ok(canonical)
    }
}

pub(super) fn snapshot(store: &Store, ctx: &TemporalContext) -> Result<Option<HashMap<i64, i64>>> {
    if !store.has_attachments() {
        return Ok(None);
    }
    read_snapshot(&store.conn, ctx)
}

fn read_snapshot(
    connection: &rusqlite::Connection,
    ctx: &TemporalContext,
) -> Result<Option<HashMap<i64, i64>>> {
    #[cfg(test)]
    if PRECEDING.get() {
        return tests::preceding_snapshot(connection, ctx);
    }
    check_eval_budget(ctx, 0, 0)?;
    // A dense subject batch visits many leaves. Do not repeatedly decode and
    // hash a prefix that a large registry makes us discard. OFFSET still
    // bounds this probe to 4,097 rows; it never counts the whole registry.
    let oversized: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM term_alias LIMIT 1 OFFSET 4096)",
        [],
        |row| row.get(0),
    )?;
    check_eval_budget(ctx, 0, 0)?;
    if oversized {
        return Ok(None);
    }
    // The exclusive connection borrow keeps this TEMP table stable during
    // the scan. A large composition keeps canonical_id's indexed lookup;
    // never use an incomplete prefix as an alias map.
    let mut statement =
        connection.prepare("SELECT alias_id, canonical_id FROM term_alias LIMIT 4097")?;
    let mut rows = statement.query([])?;
    let mut aliases = HashMap::new();
    while let Some(row) = rows.next()? {
        check_eval_budget(ctx, aliases.len(), 0)?;
        aliases.insert(row.get(0)?, row.get(1)?);
        if aliases.len() > 4096 {
            return Ok(None);
        }
    }
    Ok(Some(aliases))
}

#[cfg(test)]
#[path = "triple_alias/tests.rs"]
mod tests;
