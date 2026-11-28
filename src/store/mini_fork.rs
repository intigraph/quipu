//! Mini-Quipu slice M1 (docs/design/mini-quipu.md §2.1 and §6): the home
//! fork registry entry for a file-backed fork. Native API only.
//!
//! The entry records the fork name, the mini-Quipu `store_id` and
//! `log_epoch`, the base cut on ONE home owner (`store_id`, `log_epoch`,
//! `local_tx`, catalog revision, owner generation, policy revision), the
//! exact graph set (writable and read-only reference members), the pinned
//! shapes revision and the base digest. It is a control write on that owner,
//! never a catalog route: nothing here enters a catalog, a router or
//! `[quipu.graph_sets]`, and nothing creates or copies a store (that is M2).
//!
//! Refusals X1-X5 each write nothing and name their reason:
//! X1 the mini-Quipu identity is a home writer (a routed owner, the legacy
//! file or the owner itself); X2 part of a catalog graph set; X3 a prefix or
//! pattern member; X4 writable members on more than one home file; X5 the
//! home owner is itself a mini-Quipu.
use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use super::{GraphSetOwner, Owner};
use crate::{
    Error, Result, Store,
    graph_catalog::{GraphSetCatalog, Route},
    schema::ROOT_GRAPH_IRI,
};

const MAX_MEMBERS: usize = 256;
const TABLE: &str = "CREATE TABLE IF NOT EXISTS mini_quipu_forks (
    name     TEXT PRIMARY KEY,
    store_id TEXT NOT NULL UNIQUE,
    status   TEXT NOT NULL CHECK (status IN ('open','retired')),
    entry    TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS mini_quipu_marker (
    id            INTEGER PRIMARY KEY CHECK (id = 1),
    home_store_id TEXT NOT NULL
);";

/// The named refusals of this slice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MiniForkRefusal {
    /// The mini-Quipu identity is a writer the home already routes.
    X1RoutedWriter,
    /// Writable members are only part of a catalog graph set.
    X2PartialSet,
    /// A member is a prefix, pattern or template, not an exact IRI.
    X3PatternMember,
    /// Writable members span more than one home file.
    X4MultiFile,
    /// The home owner is itself a mini-Quipu.
    X5NestedFork,
}

impl MiniForkRefusal {
    /// Stable reason code carried in the refusal message.
    pub fn code(self) -> &'static str {
        match self {
            Self::X1RoutedWriter => "X1 routed-writer-claim",
            Self::X2PartialSet => "X2 partial-catalog-set",
            Self::X3PatternMember => "X3 prefix-or-pattern-member",
            Self::X4MultiFile => "X4 multi-file-set",
            Self::X5NestedFork => "X5 fork-from-mini-quipu",
        }
    }
}

fn refused(reason: MiniForkRefusal, detail: &str) -> Error {
    Error::PolicyDenied(format!(
        "mini-quipu fork refused ({}): {detail}",
        reason.code()
    ))
}

fn invalid(detail: &str) -> Error {
    Error::PolicyDenied(format!("mini-quipu fork refused: {detail}"))
}

/// How a declared member is held by the mini-Quipu.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MemberRole {
    /// Written in the mini-Quipu and merged back.
    Writable,
    /// A pinned read-only copy (for example an ontology or shapes graph).
    Reference,
}

/// One declared member, by exact graph IRI.
#[derive(Debug, Clone)]
pub struct MiniForkMember {
    /// Exact graph IRI. No prefix, pattern or template.
    pub iri: String,
    /// Writable or read-only reference.
    pub role: MemberRole,
}

/// A request to record a mini-Quipu fork at home.
#[derive(Debug, Clone)]
pub struct MiniForkRequest {
    /// Fork name, unique in the owner's registry.
    pub name: String,
    /// The mini-Quipu file's own store lineage (minted by M2).
    pub store_id: String,
    /// The mini-Quipu file's own change-log epoch.
    pub log_epoch: String,
    /// The complete declared graph set.
    pub members: Vec<MiniForkMember>,
    /// Ordinary registry timestamp.
    pub created_at: String,
}

/// The home cut the fork's base is taken at: one transaction coordinate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BaseCut {
    /// Home owner store lineage.
    pub store_id: String,
    /// Home owner change-log epoch.
    pub log_epoch: String,
    /// Home owner transaction at the cut.
    pub local_tx: i64,
    /// Graph-set catalog revision the set was resolved under.
    pub catalog_revision: u64,
    /// The owner's catalog generation.
    pub owner_generation: u64,
    /// Policy revision installed on the owner.
    pub policy_revision: String,
}

/// A read-only reference member and the home owner it resolves to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReferenceMember {
    /// Exact graph IRI.
    pub iri: String,
    /// Store lineage of the home owner that holds it.
    pub owner_store_id: String,
}

/// The recorded home registry entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MiniForkEntry {
    /// Fork name.
    pub name: String,
    /// Mini-Quipu store lineage.
    pub store_id: String,
    /// Mini-Quipu change-log epoch.
    pub log_epoch: String,
    /// Base cut on the one home owner.
    pub base: BaseCut,
    /// Writable members, sorted.
    pub writable: Vec<String>,
    /// Read-only reference members, sorted by IRI.
    pub references: Vec<ReferenceMember>,
    /// Pinned shapes revision at the cut (`sha256:` of the combined shapes).
    pub shapes_hash: String,
    /// Base digest over the writable members at the cut (see [`base_digest`]).
    pub base_digest: String,
    /// Registry timestamp.
    pub created_at: String,
    /// The mini-Quipu head `local_tx` of the last merge back that wrote at
    /// home (design §2.3); `None` until then. A later merge of a NEW head
    /// plans against the mini-Quipu's own state at this head.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merged_head: Option<i64>,
    /// The last merge back that wrote at home, for later merges and for the
    /// remainder of a partial merge (oracle §6).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_merge: Option<LastMerge>,
}

/// What a later merge needs from the last merge that wrote at home.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LastMerge {
    /// Its merge id; a later merge or a remainder names it.
    pub merge_id: String,
    /// Its incoming coordinate (`store_id@log_epoch#local_tx`).
    pub incoming: String,
    /// Slots it held back; a NEW incoming refuses while this is non-zero.
    pub held: usize,
    /// The base it planned against, which its remainder reuses.
    pub base: MergeBase,
    /// Digest of the mini-Quipu's live writable members at its head: the
    /// base of the next merge of a new head.
    pub head_digest: String,
}

/// The base a merge planned against.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub enum MergeBase {
    /// The registry's home cut (`base`, `base_digest`).
    Home,
    /// The mini-Quipu's own state at a merged head, with its digest.
    Mini {
        /// Mini-Quipu `local_tx`.
        head: i64,
        /// [`live_digest`] at that head.
        digest: String,
    },
}

struct Plan<'a> {
    owner: &'a Route,
    revision: u64,
    writable: Vec<String>,
    references: Vec<ReferenceMember>,
}

/// Exact IRIs only. A trailing `/`, `#` or `*`, or a template brace, is a
/// prefix or pattern form even where it would parse as an IRI.
fn exact(iri: &str) -> bool {
    !iri.is_empty()
        && !iri.contains(['*', '{', '}'])
        && !iri.ends_with(['/', '#'])
        && (iri == ROOT_GRAPH_IRI || oxrdf::NamedNode::new(iri).is_ok())
}

fn plan<'a>(catalog: &'a GraphSetCatalog, request: &MiniForkRequest) -> Result<Plan<'a>> {
    if [
        &request.name,
        &request.store_id,
        &request.log_epoch,
        &request.created_at,
    ]
    .iter()
    .any(|v| v.trim().is_empty())
    {
        return Err(invalid("name, identity, epoch and timestamp are required"));
    }
    if request.members.is_empty() || request.members.len() > MAX_MEMBERS {
        return Err(invalid("graph set is empty or exceeds its bound"));
    }
    if let Some(m) = request.members.iter().find(|m| !exact(&m.iri)) {
        return Err(refused(
            MiniForkRefusal::X3PatternMember,
            &format!("member {:?} is not an exact graph IRI", m.iri),
        ));
    }
    let physical = catalog.physical();
    if physical
        .routes()
        .chain(physical.legacy_participant())
        .any(|route| route.store_id == request.store_id)
    {
        return Err(refused(
            MiniForkRefusal::X1RoutedWriter,
            "the mini-Quipu identity is a writer the home catalog routes",
        ));
    }
    let mut seen = BTreeSet::new();
    let (mut writable, mut references) = (Vec::new(), Vec::new());
    for member in &request.members {
        if !seen.insert(member.iri.as_str()) {
            return Err(invalid("duplicate graph-set member"));
        }
        match member.role {
            MemberRole::Writable => writable.push(member.iri.clone()),
            MemberRole::Reference => references.push(member.iri.as_str()),
        }
    }
    writable.sort();
    let selected: Vec<&str> = writable.iter().map(String::as_str).collect();
    if selected.is_empty() {
        return Err(invalid("a fork needs at least one writable member"));
    }
    let groups = catalog
        .plan_read(&selected)
        .map_err(|_| invalid("a writable member is not a registered home graph"))?;
    if groups.len() != 1 {
        return Err(refused(
            MiniForkRefusal::X4MultiFile,
            "writable members span home files; no multi-file fork base exists",
        ));
    }
    let owner = groups[0].owner;
    if owner.graph != ROOT_GRAPH_IRI && catalog.owner_graphs(&owner.graph)? != selected {
        return Err(refused(
            MiniForkRefusal::X2PartialSet,
            "writable members must be the whole catalog graph set",
        ));
    }
    let mut refs = Vec::with_capacity(references.len());
    for iri in references {
        let group = catalog
            .plan_read(&[iri])
            .map_err(|_| invalid("a reference member is not a registered home graph"))?;
        refs.push(ReferenceMember {
            iri: iri.to_owned(),
            owner_store_id: group[0].owner.store_id.clone(),
        });
    }
    refs.sort_by(|a, b| a.iri.cmp(&b.iri));
    Ok(Plan {
        owner,
        revision: physical.revision(),
        writable,
        references: refs,
    })
}

fn has_table(store: &Store, table: &str) -> Result<bool> {
    Ok(store
        .conn
        .prepare("SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1")?
        .exists([table])?)
}

/// Whether this store is itself a mini-Quipu: it carries the marker M2 writes
/// when it materializes one.
pub fn is_mini_quipu(store: &Store) -> Result<bool> {
    Ok(has_table(store, "mini_quipu_marker")?
        && store
            .conn
            .prepare("SELECT 1 FROM mini_quipu_marker WHERE id = 1")?
            .exists([])?)
}

/// Mark a store as a mini-Quipu of `home_store_id`. M2 calls this when it
/// materializes the file; it is the evidence X5 reads.
pub fn mark_mini_quipu(store: &Store, home_store_id: &str) -> Result<()> {
    store.conn.execute_batch(TABLE)?;
    store.conn.execute(
        "INSERT INTO mini_quipu_marker (id, home_store_id) VALUES (1, ?1)",
        [home_store_id],
    )?;
    Ok(())
}

/// Read one registry entry back. A store with no registry has no entry.
pub fn mini_fork_entry(store: &Store, name: &str) -> Result<Option<MiniForkEntry>> {
    if !has_table(store, "mini_quipu_forks")? {
        return Ok(None);
    }
    let mut stmt = store
        .conn
        .prepare("SELECT entry FROM mini_quipu_forks WHERE name = ?1")?;
    let mut rows = stmt.query([name])?;
    match rows.next()? {
        Some(row) => serde_json::from_str(&row.get::<_, String>(0)?)
            .map(Some)
            .map_err(|e| Error::Serialization(e.to_string())),
        None => Ok(None),
    }
}

/// Number of registry entries; zero when no registry exists.
pub fn mini_fork_count(store: &Store) -> Result<i64> {
    if !has_table(store, "mini_quipu_forks")? {
        return Ok(0);
    }
    Ok(store
        .conn
        .query_row("SELECT count(*) FROM mini_quipu_forks", [], |r| r.get(0))?)
}

fn record(store: &mut Store, plan: &Plan<'_>, request: &MiniForkRequest) -> Result<MiniForkEntry> {
    store.ownership_operation(|store| {
        let tx = store.conn.unchecked_transaction()?;
        let home = store.store_id()?;
        if is_mini_quipu(store)?
            || (has_table(store, "mini_quipu_forks")?
                && store
                    .conn
                    .prepare("SELECT 1 FROM mini_quipu_forks WHERE store_id = ?1")?
                    .exists([&home])?)
        {
            return Err(refused(
                MiniForkRefusal::X5NestedFork,
                "the home owner is itself a mini-Quipu; a base is always a home cut",
            ));
        }
        if home != plan.owner.store_id || store.log_epoch()? != plan.owner.log_epoch {
            return Err(invalid("home owner identity or epoch differs from the catalog"));
        }
        if request.store_id == home {
            return Err(refused(
                MiniForkRefusal::X1RoutedWriter,
                "the mini-Quipu identity is the home owner itself",
            ));
        }
        store.conn.execute_batch(TABLE)?;
        if store
            .conn
            .prepare("SELECT 1 FROM mini_quipu_forks WHERE name = ?1 OR store_id = ?2")?
            .exists([&request.name, &request.store_id])?
        {
            return Err(invalid("fork name or mini-Quipu identity is already registered"));
        }
        let cut = store.latest_tx_id()?;
        let journals = plan
            .writable
            .iter()
            .map(|graph| member_journal(store, cut, graph))
            .collect::<Result<Vec<_>>>()?;
        let entry = MiniForkEntry {
            name: request.name.clone(),
            store_id: request.store_id.clone(),
            log_epoch: request.log_epoch.clone(),
            base: BaseCut {
                store_id: home,
                log_epoch: plan.owner.log_epoch.clone(),
                local_tx: cut,
                catalog_revision: plan.revision,
                owner_generation: plan.owner.generation,
                policy_revision: plan.owner.policy.clone(),
            },
            writable: plan.writable.clone(),
            references: plan.references.clone(),
            shapes_hash: shapes_hash(store, cut)?,
            base_digest: base_digest(&journals)?,
            created_at: request.created_at.clone(),
            merged_head: None,
            last_merge: None,
        };
        store.conn.execute(
            "INSERT INTO mini_quipu_forks (name, store_id, status, entry) VALUES (?1, ?2, 'open', ?3)",
            rusqlite::params![
                entry.name,
                entry.store_id,
                serde_json::to_string(&entry).map_err(|e| Error::Serialization(e.to_string()))?
            ],
        )?;
        tx.commit()?;
        Ok(entry)
    })
}

/// Remove the still-open registry entry of a fork whose file was never
/// completed, so its name can be used again. Matches name AND minted store
/// identity, so it can only remove the entry `record` just wrote for that file.
fn forget(store: &mut Store, entry: &MiniForkEntry) -> Result<()> {
    store.ownership_operation(|store| {
        if !has_table(store, "mini_quipu_forks")? {
            return Err(invalid("no registry entry to remove"));
        }
        let removed = store.conn.execute(
            "DELETE FROM mini_quipu_forks WHERE name = ?1 AND store_id = ?2 AND status = 'open'",
            rusqlite::params![entry.name, entry.store_id],
        )?;
        if removed != 1 {
            return Err(invalid(
                "the registry entry to remove is absent or not open",
            ));
        }
        Ok(())
    })
}

/// Remove an uncompleted fork's entry from the legacy file (see `forget`).
pub fn forget_on_legacy(legacy: &mut Store, entry: &MiniForkEntry) -> Result<()> {
    forget(legacy, entry)
}

/// Record a fork whose writable members are on the legacy file.
pub fn record_on_legacy(
    catalog: &GraphSetCatalog,
    legacy: &mut Store,
    request: &MiniForkRequest,
) -> Result<MiniForkEntry> {
    let plan = plan(catalog, request)?;
    if plan.owner.graph != ROOT_GRAPH_IRI {
        return Err(invalid(
            "writable members are routed; record on their owner",
        ));
    }
    record(legacy, &plan, request)
}

impl GraphSetOwner {
    /// Record a fork whose writable members are this owner's whole set,
    /// as a control write through this owner's one writer.
    pub fn record_mini_fork(&self, request: &MiniForkRequest) -> Result<MiniForkEntry> {
        let plan = plan(&self.catalog, request)?;
        if plan.owner != self.owner.catalog_route() {
            return Err(invalid("writable members resolve to another home owner"));
        }
        let _admission = self.owner.admit()?;
        let mut store = self.owner.writer.owned()?;
        self.owner.placement.verify()?;
        Owner::check_identity(self.owner.catalog_route(), &store)?;
        super::participant::check_write_fence(&store)?;
        record(&mut store, &plan, request)
    }

    /// Remove an uncompleted fork's entry through this owner's one writer
    /// (see `forget`).
    pub fn forget_mini_fork(&self, entry: &MiniForkEntry) -> Result<()> {
        let _admission = self.owner.admit()?;
        let mut store = self.owner.writer.owned()?;
        self.owner.placement.verify()?;
        Owner::check_identity(self.owner.catalog_route(), &store)?;
        forget(&mut store, entry)
    }

    /// Read a registry entry back on a read-only connection.
    pub fn mini_fork(&self, name: &str) -> Result<Option<MiniForkEntry>> {
        self.owner.read(|store| mini_fork_entry(store, name))
    }
}

#[path = "mini_fork_base.rs"]
mod base;
use base::shapes_hash;
pub use base::{base_digest, live_digest, member_journal};

#[path = "mini_quipu_local.rs"]
pub mod local;

#[path = "mini_quipu_merge.rs"]
pub mod merge;

#[path = "mini_quipu_retire.rs"]
pub mod retire;

#[cfg(test)]
#[path = "mini_fork_tests.rs"]
mod tests;
