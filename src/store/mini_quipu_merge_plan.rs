//! Mini-Quipu M3 planning: open `theirs` read-only and plan one member with
//! the fork path's own `Store::fork_merge`, carrying terms into the home
//! dictionary by IRI and literal value (never by term id).
use std::collections::BTreeMap;
use std::path::Path;

use rusqlite::{OptionalExtension, params};

use super::super::{MergeBase, MiniForkEntry, has_table, invalid};
use super::{MergeRefusal, refused};
use crate::{
    Result, Store,
    merge_core::selection::Mode,
    schema::ROOT_GRAPH_IRI,
    store::forks_merge::{ForkMerge, ForkResolutions},
    types::{Fact, Value},
};
/// The mini-Quipu file on a read-only connection, after step 1.
pub(super) fn open_theirs(path: &Path, entry: &MiniForkEntry) -> Result<Store> {
    let path = path
        .to_str()
        .ok_or_else(|| invalid("the mini-Quipu path is not UTF-8"))?;
    let mini = Store::open_read_only(path).map_err(|_| {
        refused(
            MergeRefusal::ForeignEpoch,
            "the mini-Quipu file cannot be read",
        )
    })?;
    let minted: Option<String> = if has_table(&mini, "mini_quipu_identity")? {
        mini.conn
            .query_row(
                "SELECT store_id FROM mini_quipu_identity WHERE id = 1",
                [],
                |r| r.get(0),
            )
            .optional()?
    } else {
        None
    };
    let own = mini.store_id()?;
    if minted.as_deref() != Some(own.as_str())
        || own != entry.store_id
        || mini.log_epoch()? != entry.log_epoch
    {
        return Err(refused(
            MergeRefusal::ForeignEpoch,
            "the file is not the mini-Quipu identity and epoch the registry records",
        ));
    }
    Ok(mini)
}

fn graph_of(store: &Store, iri: &str) -> Result<Option<i64>> {
    if iri == ROOT_GRAPH_IRI {
        return Ok(Some(crate::schema::ROOT_GRAPH));
    }
    store.committed_graph_id(iri)
}

/// A mini-Quipu value as the same RDF term in the home dictionary.
fn carry(home: &Store, mini: &Store, v: &Value) -> Result<Value> {
    match v {
        Value::Ref(id) => Ok(Value::Ref(home.intern(&mini.resolve(*id)?)?)),
        Value::Triple(_) => crate::rdf::term_to_value(home, &crate::rdf::value_to_term(mini, v)?),
        other => Ok(other.clone()),
    }
}

/// Carry a mini-Quipu fact into home terms.
fn carried(home: &Store, mini: &Store, f: Fact) -> Result<Fact> {
    Ok(Fact {
        entity: home.intern(&mini.resolve(f.entity)?)?,
        attribute: home.intern(&mini.resolve(f.attribute)?)?,
        value: carry(home, mini, &f.value)?,
        ..f
    })
}

/// Plan one writable member exactly as fork promote plans ROOT against its
/// fork graph: base at the cut (the home cut, or the mini-Quipu's own state
/// at the last merged head), ours now, theirs from the file.
pub(super) fn plan_member(
    home: &Store,
    mini: &Store,
    graph: &str,
    (base, cut): (&MergeBase, i64),
    resolved: Option<&ForkResolutions>,
    mode: Mode,
    td: Option<&str>,
) -> Result<(i64, ForkMerge)> {
    let g = graph_of(home, graph)?.ok_or_else(|| invalid("a writable member is absent at home"))?;
    let mg =
        graph_of(mini, graph)?.ok_or_else(|| invalid("a writable member is absent locally"))?;
    let theirs = mini
        .current_facts_in_graph(mg)?
        .into_iter()
        .map(|f| carried(home, mini, f))
        .collect::<Result<Vec<_>>>()?;
    let base = match base {
        MergeBase::Home => home.graph_facts_as_of(g, cut)?,
        MergeBase::Mini { head, .. } => mini
            .graph_facts_as_of(mg, *head)?
            .into_iter()
            .map(|f| carried(home, mini, f))
            .collect::<Result<Vec<_>>>()?,
    };
    let closes = |e: i64, a: i64| -> Result<BTreeMap<Vec<u8>, String>> {
        let (Some(me), Some(ma)) = (
            mini.lookup(&home.resolve(e)?)?,
            mini.lookup(&home.resolve(a)?)?,
        ) else {
            return Ok(BTreeMap::new());
        };
        let mut out = BTreeMap::new();
        let mut stmt = mini.conn.prepare_cached(
            "SELECT v, valid_to FROM facts WHERE g = ?1 AND e = ?2 AND a = ?3 AND op = 1 \
             AND valid_to IS NOT NULL ORDER BY retracted_tx DESC, rowid DESC",
        )?;
        let rows: Vec<(Vec<u8>, String)> = stmt
            .query_map(params![mg, me, ma], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        for (v, to) in rows {
            let v = carry(home, mini, &Value::from_bytes(&v)?)?;
            out.entry(v.term_key()).or_insert(to);
        }
        Ok(out)
    };
    let merge = home.fork_merge(
        base,
        home.current_facts_in_graph(g)?,
        theirs,
        resolved,
        &closes,
        mode,
        td,
    )?;
    Ok((g, merge))
}
