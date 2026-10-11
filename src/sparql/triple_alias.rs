//! A bounded, statement-local snapshot of the composed term aliases.

use std::collections::HashMap;

use super::TemporalContext;
use super::pattern_util::check_eval_budget;
use crate::{Result, Store};

pub(super) fn snapshot(store: &Store, ctx: &TemporalContext) -> Result<Option<HashMap<i64, i64>>> {
    if !store.has_attachments() {
        return Ok(None);
    }
    // The exclusive connection borrow keeps this TEMP table stable during
    // the scan. A large composition keeps canonical_id's indexed lookup;
    // never use an incomplete prefix as an alias map.
    let mut statement =
        store.prepare("SELECT alias_id, canonical_id FROM term_alias LIMIT 4097")?;
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
