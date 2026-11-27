//! Catch a stale label snapshot up from the committed transaction delta.
//!
//! The commit hook ([`super::cache::maintain`]) only sees writes made by its own
//! process, and runs after RELEASE. Two readers see a commit before it does: a
//! read worker (`src/server/process_pool.rs` routes `/resolve` there when
//! `read_processes > 0`), which never sees the primary's commits at all, and,
//! with `read_processes = 0`, a pooled reader in the primary that shares the
//! writer's cache but reads between the commit and the hook. Either one used
//! to pay a full label scan on the stale snapshot: measured (aegis-wjo7) at
//! 2.5-5 s for seeds' create path against ~0.6 s on a current snapshot. The database itself says what changed, by `idx_tx` and
//! `idx_retracted_tx`, so a stale snapshot of the SAME store generation can be
//! brought current in milliseconds instead of rebuilt.
//!
//! Anything the delta cannot certify falls back to the exact scan: an identity
//! (`owl:sameAs`) change, a different change-log epoch (a freeze deleted rows,
//! which leave no delta), a transaction id that went backwards, or a delta too
//! large to be worth patching.
use super::{
    LabelIndex, id_list,
    index::{LabelEntry, LabelProfile},
};
use crate::{error::Result, namespace, store::Store};
use std::{collections::HashSet, sync::Arc};

/// Beyond this many changed label identities a full scan is simpler and no slower.
const MAX_AFFECTED: usize = 20_000;

/// `index` (current as of `from_tx`) brought current as of at least `to_tx`, or
/// `None` when the delta cannot certify the result and the caller must scan.
///
/// The delta is read AFTER the caller stamped `to_tx`, so it covers everything
/// through `to_tx` and possibly more. Re-applying a later delta is idempotent:
/// each affected identity is rebuilt from its final current rows.
///
/// # Errors
/// Propagates store and decode failures.
pub(super) fn catch_up(
    store: &Store,
    index: &Arc<LabelIndex>,
    from_tx: i64,
    to_tx: i64,
) -> Result<Option<Arc<LabelIndex>>> {
    if to_tx < from_tx || !store.attachments.is_empty() {
        return Ok(None);
    }
    let labels = store.lookup_all(&format!("{}label", namespace::RDFS))?;
    let same_as = store.lookup_all(&format!("{}sameAs", namespace::OWL))?;
    if !same_as.is_empty() {
        let moved: bool = store.conn.query_row(
            &format!(
                "SELECT EXISTS(SELECT 1 FROM main.facts WHERE tx > {from_tx} AND +a IN ({ids})) \
                 OR EXISTS(SELECT 1 FROM main.facts WHERE retracted_tx > {from_tx} \
                 AND +a IN ({ids}))",
                ids = id_list(&same_as)
            ),
            [],
            |r| r.get(0),
        )?;
        if moved {
            return Ok(None);
        }
    }
    if labels.is_empty() {
        return Ok(Some(index.clone()));
    }
    // Asserted, explicitly retracted (op=0 row) and closed-by-supersede rows
    // all carry a tx or retracted_tx after `from_tx`; `+a` keeps each half on
    // its bounded index rather than walking every label row.
    let ids = id_list(&labels);
    let sql = format!(
        "SELECT e FROM main.facts WHERE tx > {from_tx} AND +a IN ({ids}) \
         UNION SELECT e FROM main.facts WHERE retracted_tx > {from_tx} AND +a IN ({ids}) \
         LIMIT {}",
        MAX_AFFECTED + 1
    );
    let mut stmt = store.conn.prepare(&sql)?;
    let raw = stmt
        .query_map([], |r| r.get::<_, i64>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    if raw.len() > MAX_AFFECTED {
        return Ok(None);
    }
    if raw.is_empty() {
        return Ok(Some(index.clone()));
    }
    let affected: HashSet<i64> = raw
        .iter()
        .map(|e| store.canonical_id(*e))
        .collect::<Result<_>>()?;
    Ok(Some(rebuild(store, index, &affected, &labels)?))
}

/// `index` with every entry of `affected` replaced by its current label rows,
/// including rows of other aliases of the same identity.
///
/// # Errors
/// Propagates store and decode failures.
pub(super) fn rebuild(
    store: &Store,
    index: &Arc<LabelIndex>,
    affected: &HashSet<i64>,
    labels: &[i64],
) -> Result<Arc<LabelIndex>> {
    let mut index = index.clone();
    if affected.is_empty() {
        return Ok(index);
    }
    let mut ids: Vec<i64> = affected.iter().copied().collect();
    // Intentional change from the old attached-only path: aliases are read on
    // every rebuild, because a delta touches one alias while the identity's
    // label rows may sit under another. A plain store that predates the alias
    // table has no aliases, so an unpreparable query is treated as "none";
    // only a composed store, which always carries the table, turns it into an
    // error. `separate_process_reader_catches_up_from_the_delta` and the
    // composed-store tests cover both shapes.
    let sql = format!(
        "SELECT alias_id FROM term_alias WHERE canonical_id IN ({})",
        id_list(&ids)
    );
    if let Ok(mut stmt) = store.conn.prepare(&sql) {
        let aliases = stmt
            .query_map([], |r| r.get::<_, i64>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        ids.extend(aliases);
    } else if !store.attachments.is_empty() {
        // Composed stores always carry the alias table; failing to read it
        // there means the rebuild cannot see every producer of an identity.
        return Err(crate::error::Error::InvalidValue(
            "resolution label cache: term_alias unreadable".into(),
        ));
    }
    let sql = format!(
        "SELECT e,v FROM {} WHERE a IN ({}) AND e IN ({}) AND valid_to IS NULL AND op=1",
        store.facts_source(),
        id_list(labels),
        id_list(&ids)
    );
    let mut stmt = store.conn.prepare(&sql)?;
    let mut rows = stmt.query([])?;
    let entries = Arc::make_mut(&mut index);
    entries
        .entries
        .retain(|entry| !affected.contains(&entry.entity_id));
    while let Some(row) = rows.next()? {
        let bytes: Vec<u8> = row.get(1)?;
        if let Ok(crate::types::Value::Str(label)) = crate::types::Value::from_bytes(&bytes) {
            let lower = label.to_lowercase();
            entries.entries.push(Arc::new(LabelEntry {
                entity_id: store.canonical_id(row.get(0)?)?,
                profile: LabelProfile::of(&lower),
                lower,
            }));
        }
    }
    Ok(index)
}
