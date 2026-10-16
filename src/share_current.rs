//! Pendant autoload: one stable CURRENT name per repository (aegis-koxp0u).
//!
//! `compose` never replaces an earlier snapshot, so an agent has no stable name to
//! ask for "this repository's knowledge now". Repository mode composes into
//! repository-scoped pack graphs and moves two datasets atomically:
//!
//! - `urn:quipu:repo:<repo>:current`, the latest composition that was admitted
//!   (a quarantined composition never moves it);
//! - `urn:quipu:repo:<repo>:previous`, the one before, so rollback is a pointer move.
//!
//! Older snapshot graphs this repository owns are dropped, unless another dataset
//! still names them. Local retractions are carried forward as durable per-pack
//! tombstones: a statement retracted in one snapshot stays retracted in every later
//! snapshot of the same pack, across a version where it is absent, across pruning
//! and across rollback. Blank nodes are renamed per snapshot, so a retraction of a
//! blank-node statement cannot be carried and fails closed.
//!
//! Everything happens in one savepoint. A failure anywhere leaves the pointers,
//! the graphs and the caches exactly as they were.

use std::collections::{BTreeMap, BTreeSet};

use oxrdf::{NamedOrBlankNode, Term, Triple};
use oxrdfio::{RdfFormat, RdfParser};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::share_compose::{Composition, Scope, Suppressed, canonical_line, compose_scoped};
use crate::share_import::ShareImportRequest;
use crate::store::Store;
use crate::store::datasets::DatasetMember;

/// The dataset an agent queries for a repository's current knowledge.
pub fn current_dataset(repo: &str) -> String {
    format!("urn:quipu:repo:{repo}:current")
}

/// The snapshot before CURRENT, kept so rollback is a pointer move.
pub fn previous_dataset(repo: &str) -> String {
    format!("urn:quipu:repo:{repo}:previous")
}

/// One pack graph a pointer names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PointerMember {
    pub key: String,
    pub graph: String,
    pub share_id: String,
}

/// Where one pointer points.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pointer {
    pub dataset: String,
    pub snapshot: String,
    pub members: Vec<PointerMember>,
    pub moved_at: String,
}

/// The result of a repository-mode composition.
#[derive(Debug, Serialize)]
pub struct RepoComposition {
    pub repo: String,
    /// Whether CURRENT moved. False for a quarantined composition, and for a
    /// recomposition of the snapshot CURRENT already names.
    pub moved: bool,
    pub current: Option<Pointer>,
    pub previous: Option<Pointer>,
    /// Local retractions newly recorded as tombstones before composing.
    pub tombstones_harvested: usize,
    /// Tombstoned statements retracted from reused (already composed) graphs.
    pub tombstones_reapplied: usize,
    /// Older snapshot graphs dropped by retention.
    pub dropped: Vec<String>,
    /// Older snapshot graphs kept because another dataset still names them.
    pub retained_shared: Vec<String>,
    pub composition: Composition,
}

/// The result of a rollback.
#[derive(Debug, Serialize)]
pub struct RepoRollback {
    pub repo: String,
    pub current: Pointer,
    pub previous: Pointer,
    pub tombstones_harvested: usize,
    pub tombstones_reapplied: usize,
}

/// Receipt for an explicit, repository-scoped reversal of local suppression.
#[derive(Debug, Serialize)]
pub struct TombstoneRevocation {
    pub repo: String,
    pub pack_key: String,
    pub statement: String,
    pub revoked: bool,
    pub graphs_restored: usize,
    pub reason: String,
}

/// Undo one local retraction, including its carried-forward policy.
///
/// Restore only matching composition facts already retracted in this repo's
/// owned graphs. Deleting the ledger row alone is insufficient: the next
/// composition would harvest those retractions again. CURRENT/PREVIOUS pointers
/// do not move; their matching retracted facts become visible immediately.
/// Facts omitted before a snapshot was stored are never fabricated: replaying
/// or rolling back that stripped graph stays stripped. A fresh source-pendant
/// snapshot can restore the statement after suppression is revoked.
pub fn revoke_tombstone(
    store: &mut Store,
    repo: &str,
    pack_key: &str,
    statement: &str,
    reason: &str,
    timestamp: &str,
    actor: Option<&str>,
) -> Result<TombstoneRevocation> {
    check_name("repository", repo)?;
    check_name("pack key", pack_key)?;
    if reason.trim().is_empty() || reason.len() > 1000 {
        return Err(Error::InvalidValue(
            "revocation needs a nonblank reason of at most 1000 bytes".into(),
        ));
    }
    let triples = RdfParser::from_format(RdfFormat::NTriples)
        .for_reader(statement.as_bytes())
        .map(|q| {
            q.map(Triple::from)
                .map_err(|e| Error::InvalidValue(e.to_string()))
        })
        .collect::<Result<Vec<_>>>()?;
    if triples.len() != 1 {
        return Err(Error::InvalidValue(
            "revocation requires exactly one N-Triples statement".into(),
        ));
    }
    let triple = &triples[0];
    if matches!(triple.subject, NamedOrBlankNode::BlankNode(_))
        || matches!(triple.object, Term::BlankNode(_))
    {
        return Err(Error::InvalidValue(
            "blank-node tombstones cannot be carried or revoked".into(),
        ));
    }
    let statement = format!("{triple} .");
    savepoint(store, "quipu_pendant_revoke", |store| {
        let exists = tombstones_for(store, repo, pack_key)?.contains(&statement);
        let mut restored = 0;
        {
            let graphs: Vec<String> = {
                let mut q = store.conn.prepare("SELECT graph FROM pendant_repo_graphs WHERE repo = ?1 AND pack_key = ?2 ORDER BY graph")?;
                q.query_map(params![repo, pack_key], |row| row.get(0))?
                    .collect::<std::result::Result<_, _>>()?
            };
            for graph in graphs {
                let Some(g) = store.lookup(&graph)? else {
                    continue;
                };
                let facts = store.retracted_composition_facts_in_graph(g)?;
                let bytes = crate::rdf::serialize_facts(store, &facts, RdfFormat::NTriples)?;
                let text =
                    String::from_utf8(bytes).map_err(|e| Error::Serialization(e.to_string()))?;
                let mut matching = false;
                for line in text.lines() {
                    matching |= canonical_line(line)?.as_ref() == Some(&statement);
                }
                if !matching {
                    continue;
                }
                let datum = crate::store::Datum {
                    entity: crate::rdf::intern_subject(store, &triple.subject)?,
                    attribute: store.intern(triple.predicate.as_str())?,
                    value: crate::rdf::term_to_value(store, &triple.object)?,
                    valid_from: timestamp.into(),
                    valid_to: None,
                    op: crate::types::Op::Assert,
                };
                store.transact_to_graph(
                    &[datum],
                    timestamp,
                    actor,
                    Some("pendant-current:revoke"),
                    g,
                )?;
                restored += 1;
            }
            store.conn.execute("DELETE FROM pendant_repo_tombstones WHERE repo = ?1 AND pack_key = ?2 AND statement = ?3", params![repo, pack_key, statement])?;
        }
        Ok(TombstoneRevocation {
            repo: repo.into(),
            pack_key: pack_key.into(),
            statement: statement.clone(),
            revoked: exists || restored > 0,
            graphs_restored: restored,
            reason: reason.into(),
        })
    })
}

/// A repository or pack key: path-like, no `:` (it would collide with the
/// `:pack:` and `:snapshot:` structure of the IRIs built from it).
fn check_name(kind: &str, value: &str) -> Result<()> {
    let segments_ok = value.split('/').all(|segment| {
        !segment.is_empty()
            && segment != "."
            && segment != ".."
            && segment
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
    });
    if value.is_empty() || value.len() > 200 || !segments_ok {
        return Err(Error::InvalidValue(format!(
            "{kind} {value:?} must be 1-200 characters of [A-Za-z0-9._-] in '/'-separated \
             segments (no empty, '.' or '..' segment)"
        )));
    }
    Ok(())
}

fn savepoint<T>(
    store: &mut Store,
    name: &str,
    body: impl FnOnce(&mut Store) -> Result<T>,
) -> Result<T> {
    store.conn.execute_batch(&format!("SAVEPOINT {name}"))?;
    match body(store) {
        Ok(value) => {
            store.conn.execute_batch(&format!("RELEASE {name}"))?;
            Ok(value)
        }
        Err(error) => {
            store
                .conn
                .execute_batch(&format!("ROLLBACK TO {name}; RELEASE {name}"))?;
            store.read_model.borrow_mut().clear();
            store.term_cache.borrow_mut().clear_persistent();
            Err(error)
        }
    }
}

/// Compose `requests` for `repo`, one per pack key, and move CURRENT to it.
///
/// `keys[i]` is the stable identity of `requests[i]` within the repository (its
/// path, for example): it is what makes two commits of one pack the SAME pack, so
/// it must not be inferred from the bytes.
pub fn compose_current(
    store: &mut Store,
    repo: &str,
    keys: &[String],
    requests: &[ShareImportRequest],
    authority: Option<usize>,
    timestamp: &str,
    actor: Option<&str>,
) -> Result<RepoComposition> {
    check_name("repository", repo)?;
    if keys.len() != requests.len() {
        return Err(Error::InvalidValue(
            "every pack needs exactly one pack key".into(),
        ));
    }
    let mut seen = BTreeSet::new();
    for key in keys {
        check_name("pack key", key)?;
        if !seen.insert(key.as_str()) {
            return Err(Error::InvalidValue(format!(
                "pack key {key:?} is given twice; one key names one pack"
            )));
        }
    }
    savepoint(store, "quipu_pendant_current", |store| {
        compose_current_inner(store, repo, keys, requests, authority, timestamp, actor)
    })
}

fn compose_current_inner(
    store: &mut Store,
    repo: &str,
    keys: &[String],
    requests: &[ShareImportRequest],
    authority: Option<usize>,
    timestamp: &str,
    actor: Option<&str>,
) -> Result<RepoComposition> {
    let harvested = harvest(store, repo, timestamp)?;
    let tombstones = keys
        .iter()
        .map(|key| tombstones_for(store, repo, key))
        .collect::<Result<Vec<_>>>()?;
    let scope = Scope::Repository {
        repo,
        keys,
        tombstones: &tombstones,
    };
    let (composition, suppressed) =
        compose_scoped(store, requests, authority, timestamp, actor, &scope)?;
    let reapplied = retract_statements(store, &suppressed, timestamp, actor)?;
    for (key, pack) in keys.iter().zip(&composition.packs) {
        store.conn.execute(
            "INSERT OR IGNORE INTO pendant_repo_graphs (repo, pack_key, graph, share_id, composed_at) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![repo, key, pack.graph, pack.share_id, timestamp],
        )?;
    }
    let mut current = read_pointer(store, repo, "current")?;
    let mut previous = read_pointer(store, repo, "previous")?;
    let admitted = composition.outcome == "composed";
    let same = current
        .as_ref()
        .is_some_and(|c| c.snapshot == composition.dataset);
    let moved = admitted && !same;
    let (mut dropped, mut retained_shared) = (Vec::new(), Vec::new());
    if moved {
        let members: Vec<_> = keys
            .iter()
            .zip(&composition.packs)
            .map(|(key, pack)| PointerMember {
                key: key.clone(),
                graph: pack.graph.clone(),
                share_id: pack.share_id.clone(),
            })
            .collect();
        if let Some(old) = current.take() {
            previous = Some(write_pointer(
                store,
                repo,
                "previous",
                &old.snapshot,
                &old.members,
                timestamp,
                actor,
            )?);
        }
        current = Some(write_pointer(
            store,
            repo,
            "current",
            &composition.dataset,
            &members,
            timestamp,
            actor,
        )?);
        (dropped, retained_shared) = retain(
            store,
            repo,
            current.as_ref(),
            previous.as_ref(),
            timestamp,
            actor,
        )?;
    }
    Ok(RepoComposition {
        repo: repo.into(),
        moved,
        current,
        previous,
        tombstones_harvested: harvested,
        tombstones_reapplied: reapplied,
        dropped,
        retained_shared,
        composition,
    })
}

/// Swap CURRENT and previous. Tombstones recorded since previous was current are
/// applied to its graphs first, so a rollback never resurrects a retraction.
pub fn rollback_current(
    store: &mut Store,
    repo: &str,
    timestamp: &str,
    actor: Option<&str>,
) -> Result<RepoRollback> {
    check_name("repository", repo)?;
    savepoint(store, "quipu_pendant_rollback", |store| {
        let harvested = harvest(store, repo, timestamp)?;
        let current = read_pointer(store, repo, "current")?.ok_or_else(|| {
            Error::InvalidValue(format!("repository {repo:?} has no CURRENT snapshot"))
        })?;
        let previous = read_pointer(store, repo, "previous")?.ok_or_else(|| {
            Error::InvalidValue(format!(
                "repository {repo:?} has no previous snapshot to roll back to"
            ))
        })?;
        let mut suppressed: Suppressed = Vec::new();
        for member in &previous.members {
            let dead = tombstones_for(store, repo, &member.key)?;
            if dead.is_empty() {
                continue;
            }
            let (bytes, _) =
                crate::rdf::export_rdf_subset(store, RdfFormat::NTriples, Some(&member.graph))?;
            let text = String::from_utf8(bytes).map_err(|e| Error::Serialization(e.to_string()))?;
            let live: Vec<String> = text
                .lines()
                .filter_map(|line| canonical_line(line).transpose())
                .collect::<Result<Vec<_>>>()?
                .into_iter()
                .filter(|line| dead.contains(line))
                .collect();
            if !live.is_empty() {
                suppressed.push((member.graph.clone(), live));
            }
        }
        let reapplied = retract_statements(store, &suppressed, timestamp, actor)?;
        let new_current = write_pointer(
            store,
            repo,
            "current",
            &previous.snapshot,
            &previous.members,
            timestamp,
            actor,
        )?;
        let new_previous = write_pointer(
            store,
            repo,
            "previous",
            &current.snapshot,
            &current.members,
            timestamp,
            actor,
        )?;
        Ok(RepoRollback {
            repo: repo.into(),
            current: new_current,
            previous: new_previous,
            tombstones_harvested: harvested,
            tombstones_reapplied: reapplied,
        })
    })
}

/// Where `repo`'s `slot` pointer points, if it has ever moved.
pub fn read_pointer(store: &Store, repo: &str, slot: &str) -> Result<Option<Pointer>> {
    let row: Option<(String, String, String)> = store
        .conn
        .query_row(
            "SELECT snapshot, members, moved_at FROM pendant_repo_pointers WHERE repo = ?1 AND slot = ?2",
            params![repo, slot],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    row.map(|(snapshot, members, moved_at)| {
        Ok(Pointer {
            dataset: if slot == "current" {
                current_dataset(repo)
            } else {
                previous_dataset(repo)
            },
            snapshot,
            members: serde_json::from_str(&members)
                .map_err(|e| Error::Serialization(e.to_string()))?,
            moved_at,
        })
    })
    .transpose()
}

fn write_pointer(
    store: &mut Store,
    repo: &str,
    slot: &str,
    snapshot: &str,
    members: &[PointerMember],
    timestamp: &str,
    actor: Option<&str>,
) -> Result<Pointer> {
    let dataset = if slot == "current" {
        current_dataset(repo)
    } else {
        previous_dataset(repo)
    };
    let dataset_members: Vec<_> = members
        .iter()
        .map(|m| DatasetMember::new(&m.graph))
        .collect();
    store.dataset_create(&dataset, &dataset_members, timestamp, actor)?;
    let encoded =
        serde_json::to_string(members).map_err(|e| Error::Serialization(e.to_string()))?;
    store.conn.execute(
        "INSERT OR REPLACE INTO pendant_repo_pointers (repo, slot, snapshot, members, moved_at) \
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![repo, slot, snapshot, encoded, timestamp],
    )?;
    Ok(Pointer {
        dataset,
        snapshot: snapshot.into(),
        members: members.to_vec(),
        moved_at: timestamp.into(),
    })
}

/// Durable tombstones for one pack, as canonical N-Triples lines.
fn tombstones_for(store: &Store, repo: &str, key: &str) -> Result<BTreeSet<String>> {
    let mut stmt = store.conn.prepare(
        "SELECT statement FROM pendant_repo_tombstones WHERE repo = ?1 AND pack_key = ?2",
    )?;
    let rows = stmt.query_map(params![repo, key], |r| r.get::<_, String>(0))?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

/// Record every local retraction in `repo`'s owned graphs as a tombstone.
///
/// Returns how many were new. A retracted statement with a blank node refuses the
/// whole operation: blank nodes are renamed per snapshot, so there is no later
/// statement it could be matched against, and dropping it silently would bring
/// the retracted fact back with the next commit.
fn harvest(store: &Store, repo: &str, timestamp: &str) -> Result<usize> {
    let owned: Vec<(String, String)> = {
        let mut stmt = store.conn.prepare(
            "SELECT pack_key, graph FROM pendant_repo_graphs WHERE repo = ?1 ORDER BY graph",
        )?;
        let rows = stmt.query_map(params![repo], |r| Ok((r.get(0)?, r.get(1)?)))?;
        rows.collect::<std::result::Result<_, _>>()?
    };
    let mut recorded = 0;
    for (key, graph) in owned {
        let Some(g) = store.lookup(&graph)? else {
            continue;
        };
        let facts = store.retracted_composition_facts_in_graph(g)?;
        if facts.is_empty() {
            continue;
        }
        let bytes = crate::rdf::serialize_facts(store, &facts, RdfFormat::NTriples)?;
        let text = String::from_utf8(bytes).map_err(|e| Error::Serialization(e.to_string()))?;
        for quad in RdfParser::from_format(RdfFormat::NTriples).for_reader(text.as_bytes()) {
            let triple = Triple::from(quad.map_err(|e| Error::InvalidValue(e.to_string()))?);
            let blank = matches!(triple.subject, NamedOrBlankNode::BlankNode(_))
                || matches!(triple.object, Term::BlankNode(_));
            let statement = format!("{triple} .");
            if blank {
                return Err(Error::InvalidValue(format!(
                    "pack {key:?} of repository {repo:?} has a local retraction of a blank-node \
                     statement in {graph}: {statement} Blank nodes are renamed per snapshot, so it \
                     cannot be carried forward. Re-assert it there, or give the node an IRI in \
                     the pack."
                )));
            }
            recorded += store.conn.execute(
                "INSERT OR IGNORE INTO pendant_repo_tombstones (repo, pack_key, statement, recorded_at) \
                 VALUES (?1, ?2, ?3, ?4)",
                params![repo, key, statement, timestamp],
            )?;
        }
    }
    Ok(recorded)
}

/// Retract canonical statements from the graphs that still hold them.
fn retract_statements(
    store: &mut Store,
    suppressed: &Suppressed,
    timestamp: &str,
    actor: Option<&str>,
) -> Result<usize> {
    let mut retracted = 0;
    for (graph, lines) in suppressed {
        let g = store
            .lookup(graph)?
            .ok_or_else(|| Error::InvalidValue(format!("unknown graph: {graph}")))?;
        let mut datums = Vec::new();
        for line in lines {
            for quad in RdfParser::from_format(RdfFormat::NTriples).for_reader(line.as_bytes()) {
                let triple = Triple::from(quad.map_err(|e| Error::InvalidValue(e.to_string()))?);
                datums.push(crate::store::Datum {
                    entity: crate::rdf::intern_subject(store, &triple.subject)?,
                    attribute: store.intern(triple.predicate.as_str())?,
                    value: crate::rdf::term_to_value(store, &triple.object)?,
                    valid_from: timestamp.to_string(),
                    valid_to: None,
                    op: crate::types::Op::Retract,
                });
            }
        }
        if datums.is_empty() {
            continue;
        }
        retracted += datums.len();
        store.transact_to_graph(
            &datums,
            timestamp,
            actor,
            Some("pendant-current:tombstone"),
            g,
        )?;
    }
    Ok(retracted)
}

/// Drop `repo`'s snapshot graphs and datasets that neither pointer names.
///
/// A graph another dataset still names is kept (and reported), never dropped
/// from under it; its ownership row stays so a later retention can drop it once
/// it is free.
fn retain(
    store: &mut Store,
    repo: &str,
    current: Option<&Pointer>,
    previous: Option<&Pointer>,
    timestamp: &str,
    actor: Option<&str>,
) -> Result<(Vec<String>, Vec<String>)> {
    let live_snapshots: BTreeSet<&str> = current
        .into_iter()
        .chain(previous)
        .map(|p| p.snapshot.as_str())
        .collect();
    let live_graphs: BTreeSet<&str> = current
        .into_iter()
        .chain(previous)
        .flat_map(|p| p.members.iter().map(|m| m.graph.as_str()))
        .collect();
    let prefix = format!("urn:quipu:repo:{repo}:snapshot:");
    let snapshots: Vec<String> = {
        let mut stmt = store
            .conn
            .prepare("SELECT name FROM datasets WHERE substr(name, 1, length(?1)) = ?1")?;
        let rows = stmt.query_map(params![prefix], |r| r.get(0))?;
        rows.collect::<std::result::Result<_, _>>()?
    };
    for name in snapshots {
        if !live_snapshots.contains(name.as_str()) {
            store.dataset_remove(&name)?;
        }
    }
    let owned: Vec<String> = {
        let mut stmt = store
            .conn
            .prepare("SELECT graph FROM pendant_repo_graphs WHERE repo = ?1 ORDER BY graph")?;
        let rows = stmt.query_map(params![repo], |r| r.get(0))?;
        rows.collect::<std::result::Result<_, _>>()?
    };
    let (mut dropped, mut shared) = (Vec::new(), Vec::new());
    for graph in owned {
        if live_graphs.contains(graph.as_str()) {
            continue;
        }
        let named_elsewhere: Option<String> = store
            .conn
            .query_row(
                "SELECT dataset FROM dataset_members WHERE graph_iri = ?1 LIMIT 1",
                params![graph],
                |r| r.get(0),
            )
            .optional()?;
        if named_elsewhere.is_some() {
            shared.push(graph);
            continue;
        }
        let registered = store
            .lookup(&graph)?
            .is_some_and(|g| store.graph_class(g).ok().flatten().is_some());
        if registered {
            crate::delete_rdf_graph(store, timestamp, actor, Some(&graph))?;
        }
        store.conn.execute(
            "DELETE FROM pendant_repo_graphs WHERE repo = ?1 AND graph = ?2",
            params![repo, graph],
        )?;
        dropped.push(graph);
    }
    Ok((dropped, shared))
}

/// Grouped tombstone counts per pack, for diagnostics.
pub fn tombstone_counts(store: &Store, repo: &str) -> Result<BTreeMap<String, usize>> {
    let mut stmt = store.conn.prepare(
        "SELECT pack_key, COUNT(*) FROM pendant_repo_tombstones WHERE repo = ?1 GROUP BY pack_key",
    )?;
    let rows = stmt.query_map(params![repo], |r| Ok((r.get(0)?, r.get::<_, i64>(1)?)))?;
    rows.map(|row| {
        let (key, n): (String, i64) = row?;
        Ok((key, usize::try_from(n).unwrap_or(0)))
    })
    .collect()
}

#[cfg(all(test, feature = "shacl"))]
#[path = "share_current_tests.rs"]
mod tests;
