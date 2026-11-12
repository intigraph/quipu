//! Shape-aware three-way reconnect for git-native knowledge shares.

#![cfg(not(target_arch = "wasm32"))]

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Path, PathBuf};

use oxrdf::Triple;
use oxrdfio::{RdfFormat, RdfParser};
use serde::Serialize;

use crate::error::{Error, Result};
use crate::share::{ShareManifest, manifest_bytes, sha256};
use crate::store::Store;
#[path = "share_merge_core.rs"]
pub(crate) mod core;

pub(crate) type Graph = HashSet<Triple>;
type Slot = (String, String);

#[derive(Clone)]
pub(crate) struct LoadedShare {
    pub(crate) dir: PathBuf,
    pub(crate) manifest: ShareManifest,
    pub(crate) graph: Graph,
    pub(crate) shapes: String,
}

pub use crate::merge3::DecisionRecord;

#[derive(Debug, Clone, Serialize)]
pub struct ShareStatus {
    pub incoming_share: String,
    pub base_share: String,
    pub base_path: String,
    pub local_graph_hash: String,
    pub incoming_graph_hash: String,
    pub ours_added: usize,
    pub ours_removed: usize,
    pub theirs_added: usize,
    pub theirs_removed: usize,
    pub conflicts: Vec<DecisionRecord>,
    pub diverged: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct MergeResult {
    pub outcome: String,
    pub tx_id: Option<i64>,
    pub asserted: usize,
    pub retracted: usize,
    pub provenance_parents: [String; 2],
    pub conflicts: Vec<DecisionRecord>,
}

pub(crate) fn parse_graph(input: &str, what: &str) -> Result<Graph> {
    RdfParser::from_format(RdfFormat::NTriples)
        .for_reader(input.as_bytes())
        .map(|q| {
            q.map(Triple::from)
                .map_err(|e| Error::InvalidValue(format!("{what} parse: {e}")))
        })
        .collect()
}

pub(crate) fn read_share(dir: &Path) -> Result<LoadedShare> {
    let read = |name: &str| {
        std::fs::read_to_string(dir.join(name))
            .map_err(|e| Error::Store(format!("share read {}/{name}: {e}", dir.display())))
    };
    let manifest: ShareManifest = serde_json::from_str(&read("manifest.json")?)
        .map_err(|e| Error::Serialization(format!("share manifest: {e}")))?;
    share_from_parts(dir, manifest, &read("export.nt")?, read("shapes.ttl")?)
}

/// A share delivered inline (REST / MCP), checked exactly as one read from
/// disk: envelope schema, share id, and both payload hashes (aegis-yavo9c).
pub(crate) fn share_from_parts(
    dir: &Path,
    manifest: ShareManifest,
    export: &str,
    shapes: String,
) -> Result<LoadedShare> {
    crate::share_fluent::validate(&manifest.fluent_proofs)?;
    crate::fluent::pending_config::storage::validate_requests(&manifest.observable_configs)?;
    crate::competency::carriage::refuse_unsupported(&manifest, "share merge")?;
    let expected_share_id = sha256(&manifest_bytes(&manifest, false)?);
    if manifest.schema != "https://github.com/scbrown/quipu/share-manifest/v1"
        || manifest.files.graph != "export.nt"
        || manifest.files.shapes != "shapes.ttl"
        || manifest.share_id != expected_share_id
        || manifest.graph_hash != sha256(export.as_bytes())
        || manifest.shapes_hash != sha256(shapes.as_bytes())
    {
        return Err(Error::InvalidValue(format!(
            "share envelope or payload hash mismatch: {}",
            dir.display()
        )));
    }
    Ok(LoadedShare {
        dir: dir.into(),
        manifest,
        graph: parse_graph(export, "share export.nt")?,
        shapes,
    })
}

fn scan(root: &Path, depth: usize, out: &mut Vec<PathBuf>) -> Result<()> {
    if depth > 5 || !root.is_dir() {
        return Ok(());
    }
    if root.join("manifest.json").is_file() {
        out.push(root.into());
        return Ok(());
    }
    for entry in std::fs::read_dir(root)
        .map_err(|e| Error::Store(format!("lineage scan {}: {e}", root.display())))?
    {
        let path = entry
            .map_err(|e| Error::Store(format!("lineage entry: {e}")))?
            .path();
        if path.is_dir() && path.file_name().is_none_or(|n| n != ".git") {
            scan(&path, depth + 1, out)?;
        }
    }
    Ok(())
}

pub(crate) fn locate_base(incoming: &LoadedShare) -> Result<LoadedShare> {
    let parent = incoming.manifest.parent_share.as_deref().ok_or_else(|| {
        Error::InvalidValue(
            "incoming share has no parent_share; three-way merge has no base".into(),
        )
    })?;
    let root = incoming.dir.parent().unwrap_or(Path::new("."));
    let mut dirs = Vec::new();
    scan(root, 0, &mut dirs)?;
    let mut matches: Vec<_> = dirs
        .into_iter()
        .filter(|d| d != &incoming.dir)
        .filter_map(|d| read_share(&d).ok())
        .filter(|s| s.manifest.share_id == parent)
        .collect();
    match matches.len() {
        1 => Ok(matches.remove(0)),
        0 => Err(Error::InvalidValue(format!(
            "base share {parent} not found under {}",
            root.display()
        ))),
        n => Err(Error::InvalidValue(format!(
            "base share {parent} is ambiguous: found {n} copies under {}",
            root.display()
        ))),
    }
}

pub(crate) fn root_graph(store: &Store) -> Result<(Graph, String)> {
    let (bytes, _) = crate::rdf::export_rdf_subset(store, RdfFormat::NTriples, None)?;
    let hash = sha256(&bytes);
    let text = String::from_utf8(bytes)
        .map_err(|e| Error::Serialization(format!("ROOT export is not UTF-8: {e}")))?;
    Ok((parse_graph(&text, "ROOT export")?, hash))
}

pub(crate) fn by_slot(graph: &Graph) -> BTreeMap<Slot, BTreeSet<String>> {
    let mut out = BTreeMap::new();
    for t in graph {
        out.entry((t.subject.to_string(), t.predicate.as_str().to_string()))
            .or_insert_with(BTreeSet::new)
            .insert(t.object.to_string());
    }
    out
}

pub(crate) fn merge_graphs(
    base: &Graph,
    ours: &Graph,
    theirs: &Graph,
    shapes: &str,
) -> Result<(Graph, Vec<DecisionRecord>)> {
    let (b, o, t) = (by_slot(base), by_slot(ours), by_slot(theirs));
    let bounds = crate::merge3::ShapeBounds::parse(shapes)?;
    let view = if bounds.is_empty() {
        crate::merge3::TripleView::default()
    } else {
        crate::merge3::TripleView::new(base.iter().chain(ours).chain(theirs))
    };
    let empty = BTreeSet::new();
    let (slots, held) = crate::merge3::merge_slots(
        &b,
        &o,
        &t,
        |slot| bounds.bound(&slot.0, &slot.1, &view),
        &empty,
    )?;
    let conflicts = held
        .into_iter()
        .map(|c| DecisionRecord {
            subject: c.slot.0.clone(),
            predicate: c.slot.1.clone(),
            max_count: c.max_count,
            base: c.base.iter().cloned().collect(),
            ours: c.ours.iter().cloned().collect(),
            theirs: c.theirs.iter().cloned().collect(),
        })
        .collect();
    let mut merged = Graph::new();
    for (slot, values) in slots {
        for value in values {
            let line = format!("{} <{}> {} .", slot.0, slot.1, value);
            merged.extend(parse_graph(&line, "merged triple")?);
        }
    }
    Ok((merged, conflicts))
}

fn parts(
    store: &Store,
    incoming_dir: &Path,
) -> Result<(
    ShareStatus,
    Graph,
    Graph,
    String,
    String,
    [ShareManifest; 2],
)> {
    let incoming = read_share(incoming_dir)?;
    let base = locate_base(&incoming)?;
    let carriers = [base.manifest.clone(), incoming.manifest.clone()];
    let (ours, ours_hash) = root_graph(store)?;
    let (_, conflicts) = merge_graphs(&base.graph, &ours, &incoming.graph, &incoming.shapes)?;
    let status = ShareStatus {
        incoming_share: incoming.manifest.share_id.clone(),
        base_share: base.manifest.share_id,
        base_path: base.dir.display().to_string(),
        local_graph_hash: ours_hash,
        incoming_graph_hash: incoming.manifest.graph_hash,
        ours_added: ours.difference(&base.graph).count(),
        ours_removed: base.graph.difference(&ours).count(),
        theirs_added: incoming.graph.difference(&base.graph).count(),
        theirs_removed: base.graph.difference(&incoming.graph).count(),
        diverged: ours != base.graph && incoming.graph != base.graph,
        conflicts,
    };
    Ok((
        status,
        base.graph,
        incoming.graph,
        incoming.shapes,
        incoming.manifest.created_at,
        carriers,
    ))
}

pub fn status(store: &Store, incoming_dir: &Path) -> Result<ShareStatus> {
    parts(store, incoming_dir).map(|p| p.0)
}

pub fn merge(
    store: &mut Store,
    incoming_dir: &Path,
    timestamp: &str,
    actor: Option<&str>,
) -> Result<MergeResult> {
    core::atomic(store, |store| {
        merge_inner(store, incoming_dir, timestamp, actor)
    })
}

fn merge_inner(
    store: &mut Store,
    incoming_dir: &Path,
    timestamp: &str,
    actor: Option<&str>,
) -> Result<MergeResult> {
    let (status, base, theirs, shapes, authored_at, carriers) = parts(store, incoming_dir)?;
    let (ours, _) = root_graph(store)?;
    let (merged, conflicts) = merge_graphs(&base, &ours, &theirs, &shapes)?;
    let parents = [status.local_graph_hash, status.incoming_share];
    let id = crate::merge_core::merge_id("share", &parents[1], &status.base_share, &sha256(b"[]"));
    if let Some(tx) = store.find_transaction_by_source(&format!("merge_id={id}"))? {
        return Ok(MergeResult {
            outcome: "already-applied".into(),
            tx_id: Some(tx),
            asserted: 0,
            retracted: 0,
            provenance_parents: parents,
            conflicts: Vec::new(),
        });
    }
    if !conflicts.is_empty() {
        return Ok(MergeResult {
            outcome: "conflicts".into(),
            tx_id: None,
            asserted: 0,
            retracted: 0,
            provenance_parents: parents,
            conflicts,
        });
    }
    let source = format!(
        "share-merge:parents={},{};merge_id={id}",
        parents[0], parents[1]
    );
    let plan = core::plan(
        store,
        &core::Inputs {
            base: &base,
            ours: &ours,
            theirs: &theirs,
            shapes: &shapes,
            authored_at: &authored_at,
        },
        &BTreeMap::new(),
        timestamp,
    )?;
    // Pending content follows the exact already-verified parent manifests;
    // ordinary reconnect never invokes native event admission. The outer
    // savepoint rolls these rows back if the normal RDF writer refuses.
    for manifest in &carriers {
        crate::share_fluent::retain(store, manifest)?;
    }
    commit(
        store, &ours, &merged, &plan, parents, &source, timestamp, actor,
    )
}

/// Persist the common core’s ordered receive/decide plan and report final changes.
#[allow(clippy::too_many_arguments)]
pub(crate) fn commit(
    store: &mut Store,
    ours: &Graph,
    merged: &Graph,
    plan: &core::Plan,
    parents: [String; 2],
    source: &str,
    timestamp: &str,
    actor: Option<&str>,
) -> Result<MergeResult> {
    let additions: Graph = merged.difference(ours).cloned().collect();
    let removals: Graph = ours.difference(merged).cloned().collect();
    let render = |g: &Graph| {
        let mut lines: Vec<_> = g.iter().map(|t| format!("{t} .\n")).collect();
        lines.sort();
        lines.concat()
    };
    // aegis-gxfm: write-time SHACL judges the additions inside `transact`; a
    // merge (or a decision) that only REMOVES values needs the deletion gate.
    store.enforce_retraction_shapes_nt(
        &render(&removals),
        &render(&additions),
        crate::schema::ROOT_GRAPH,
        actor,
        Some(source),
    )?;
    let tx_id = core::persist(store, plan, timestamp, actor, source)?;
    Ok(MergeResult {
        outcome: "merged".into(),
        tx_id: Some(tx_id),
        asserted: additions.len(),
        retracted: removals.len(),
        provenance_parents: parents,
        conflicts: Vec::new(),
    })
}

#[cfg(test)]
#[path = "share_merge_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "share_merge_scope_tests.rs"]
mod scope_tests;
