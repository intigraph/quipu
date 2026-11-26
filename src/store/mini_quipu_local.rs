//! Mini-Quipu slice M2 (docs/design/mini-quipu.md §2.1 step 4 and §2.2):
//! materialize the mini-Quipu file and write in it locally. Native API.
//!
//! Materializing creates a NEW file, so the store mints a fresh `store_id`
//! and `log_epoch`; a byte copy of a home file keeps the home identity and is
//! never accepted ([`MiniQuipu::open`]). Every declared member is copied as
//! of the recorded cut with its valid time, the way `fork_create` copies a
//! same-store fork (one materializing transaction, raw rows, no re-judging of
//! history). The home shapes at the cut are pinned into the file, and the
//! cut's policy revision is recorded with the identity.
//!
//! Local writes are the ordinary write pipeline on that file. Two refusals
//! are added, each writing nothing:
//! X6 a write to a read-only reference member (also a SQL backstop, so every
//! write path on the file refuses, not only this wrapper);
//! X8 a cursor or `--at` position whose store identity is not the store it is
//! used on: a home cursor locally, or a local cursor at home.
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::{MiniForkEntry, MiniForkRequest, invalid, member_journal};
use crate::{Error, Result, Store, changeset::Statement, schema::ROOT_GRAPH_IRI, store::Datum};

const PINNED_SHAPES: &str = "mini-quipu-pinned-home-shapes";
const IDENTITY: &str = "CREATE TABLE IF NOT EXISTS mini_quipu_identity (
    id       INTEGER PRIMARY KEY CHECK (id = 1),
    store_id TEXT NOT NULL,
    entry    TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS mini_quipu_references (g INTEGER PRIMARY KEY);
CREATE TRIGGER IF NOT EXISTS mini_quipu_x6_insert BEFORE INSERT ON facts
WHEN NEW.g IN (SELECT g FROM mini_quipu_references) BEGIN
    SELECT RAISE(ABORT, 'mini-quipu write refused (X6 read-only-reference-member)');
END;
CREATE TRIGGER IF NOT EXISTS mini_quipu_x6_update BEFORE UPDATE ON facts
WHEN OLD.g IN (SELECT g FROM mini_quipu_references) BEGIN
    SELECT RAISE(ABORT, 'mini-quipu write refused (X6 read-only-reference-member)');
END;";

/// The local refusals of this slice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalRefusal {
    /// A write to a read-only reference member.
    X6ReferenceWrite,
    /// A cursor whose store identity is not this store's.
    X8ForeignCursor,
}

impl LocalRefusal {
    /// Stable reason code carried in the refusal message.
    pub fn code(self) -> &'static str {
        match self {
            Self::X6ReferenceWrite => "X6 read-only-reference-member",
            Self::X8ForeignCursor => "X8 foreign-cursor",
        }
    }
}

fn refused(reason: LocalRefusal, detail: &str) -> Error {
    Error::PolicyDenied(format!(
        "mini-quipu write refused ({}): {detail}",
        reason.code()
    ))
}

/// A history position that names the store it belongs to (design B4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StoreCursor {
    /// Store lineage the position belongs to.
    pub store_id: String,
    /// Change-log epoch the position belongs to.
    pub log_epoch: String,
    /// Transaction on that store.
    pub tx: i64,
}

impl StoreCursor {
    /// The current head of `store`, as a cursor.
    pub fn head(store: &Store) -> Result<Self> {
        Ok(Self {
            store_id: store.store_id()?,
            log_epoch: store.log_epoch()?,
            tx: store.latest_tx_id()?,
        })
    }

    /// Admit this cursor on `store`, or refuse X8. Home uses this to refuse
    /// mini-Quipu cursors; the mini-Quipu uses it to refuse home cursors.
    pub fn admit(&self, store: &Store) -> Result<crate::store::AsOf> {
        if self.store_id != store.store_id()? || self.log_epoch != store.log_epoch()? {
            return Err(refused(
                LocalRefusal::X8ForeignCursor,
                "the cursor names another store or epoch; positions are not portable",
            ));
        }
        if self.tx < 0 || self.tx > store.latest_tx_id()? {
            return Err(invalid("cursor transaction is outside this store's log"));
        }
        Ok(crate::store::AsOf {
            tx: Some(self.tx),
            valid_at: None,
        })
    }
}

/// What a materialization needs besides the home owner: the fork name, the
/// declared graph set and a timestamp. The mini-Quipu identity is minted by
/// the new file, never supplied.
#[derive(Debug, Clone)]
pub struct MaterializeSpec {
    /// Fork name, unique in the owner's registry.
    pub name: String,
    /// The complete declared graph set.
    pub members: Vec<super::MiniForkMember>,
    /// Registry and materialization timestamp.
    pub created_at: String,
}

/// An opened, accepted mini-Quipu file.
pub struct MiniQuipu {
    store: Store,
    entry: MiniForkEntry,
}

fn graph_id(store: &Store, iri: &str) -> Result<i64> {
    if iri == ROOT_GRAPH_IRI {
        return Ok(0);
    }
    match store.committed_graph_id(iri)? {
        Some(g) => Ok(g),
        None => store.graph_create(iri),
    }
}

fn copy_member(home: &Store, dest: &Store, cut: i64, iri: &str, tx: i64) -> Result<usize> {
    let journal = member_journal(home, cut, iri)?;
    let g = graph_id(dest, iri)?;
    for row in journal.rows() {
        let quad = Statement::new(&row.triple, None).to_quad()?;
        let oxrdf::NamedOrBlankNode::NamedNode(subject) = &quad.subject else {
            return Err(invalid(
                "blank-node subjects are not copied into a mini-Quipu",
            ));
        };
        let value = crate::rdf::term_to_value(dest, &quad.object)?;
        dest.conn.execute(
            "INSERT INTO facts (e, a, v, g, tx, valid_from, valid_to, op) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 1)",
            rusqlite::params![
                dest.intern(subject.as_str())?,
                dest.intern(quad.predicate.as_str())?,
                value.to_bytes(),
                g,
                tx,
                row.valid_from,
                row.valid_to
            ],
        )?;
    }
    Ok(journal.rows().len())
}

/// Copy every declared member of `entry` from `home` at the recorded cut into
/// the fresh `dest`, pin the cut's shapes, install the X6 backstop and the
/// identity row, then prove the copied writable members hash to the entry's
/// base digest.
fn fill(home: &Store, dest: &mut Store, entry: &MiniForkEntry) -> Result<()> {
    let cut = entry.base.local_tx;
    let as_of = crate::store::AsOf {
        tx: Some(cut),
        valid_at: None,
    };
    if let Some(shapes) = home.get_combined_shapes_as_of(&as_of)? {
        dest.load_shapes(PINNED_SHAPES, &shapes, &entry.created_at)?;
    }
    if super::base::shapes_hash(dest, dest.latest_tx_id()?)? != entry.shapes_hash {
        return Err(invalid(
            "pinned shapes differ from the recorded shapes revision",
        ));
    }
    let tx = dest.conn.unchecked_transaction()?;
    dest.conn.execute(
        "INSERT INTO transactions (timestamp, actor, source) VALUES (?1, NULL, ?2)",
        rusqlite::params![entry.created_at, format!("mini-quipu:{}", entry.name)],
    )?;
    let mat_tx = dest.conn.last_insert_rowid();
    crate::transaction_auth::record(&dest.conn, mat_tx)?;
    for iri in &entry.writable {
        copy_member(home, dest, cut, iri, mat_tx)?;
    }
    dest.conn.execute_batch(IDENTITY)?;
    for reference in &entry.references {
        copy_member(home, dest, cut, &reference.iri, mat_tx)?;
        dest.conn.execute(
            "INSERT OR IGNORE INTO mini_quipu_references (g) VALUES (?1)",
            [graph_id(dest, &reference.iri)?],
        )?;
    }
    dest.conn.execute(
        "INSERT INTO mini_quipu_identity (id, store_id, entry) VALUES (1, ?1, ?2)",
        rusqlite::params![
            entry.store_id,
            serde_json::to_string(entry).map_err(|e| Error::Serialization(e.to_string()))?
        ],
    )?;
    tx.commit()?;
    super::mark_mini_quipu(dest, &entry.base.store_id)?;
    let head = dest.latest_tx_id()?;
    let journals = entry
        .writable
        .iter()
        .map(|g| member_journal(dest, head, g))
        .collect::<Result<Vec<_>>>()?;
    if super::base_digest(&journals)? != entry.base_digest {
        return Err(invalid(
            "copied members do not hash to the recorded base digest",
        ));
    }
    Ok(())
}

fn remove_file(path: &Path) {
    for suffix in ["", "-wal", "-shm", "-journal"] {
        let mut name = path.as_os_str().to_owned();
        name.push(suffix);
        let _ = std::fs::remove_file(name);
    }
}

/// Create the fresh file, let `record` write the home registry entry under
/// the file's own minted identity, then let `fill` copy at the cut. Any
/// failure removes the new file; a home refusal (X1-X5) writes nothing.
fn materialize(
    path: &Path,
    spec: &MaterializeSpec,
    record: impl FnOnce(&MiniForkRequest) -> Result<MiniForkEntry>,
    fill: impl FnOnce(&mut Store, &MiniForkEntry) -> Result<()>,
) -> Result<MiniQuipu> {
    if path.exists() {
        return Err(invalid("the mini-Quipu path already exists"));
    }
    let path_str = path
        .to_str()
        .ok_or_else(|| invalid("the mini-Quipu path is not UTF-8"))?;
    let result = (|| {
        let mut store = Store::open(path_str)?;
        let request = MiniForkRequest {
            name: spec.name.clone(),
            store_id: store.store_id()?,
            log_epoch: store.log_epoch()?,
            members: spec.members.clone(),
            created_at: spec.created_at.clone(),
        };
        let entry = record(&request)?;
        fill(&mut store, &entry)?;
        Ok(MiniQuipu { store, entry })
    })();
    if result.is_err() {
        remove_file(path);
    }
    result
}

/// Materialize a mini-Quipu whose writable members are on the legacy file.
pub fn materialize_on_legacy(
    catalog: &crate::graph_catalog::GraphSetCatalog,
    home: &mut Store,
    spec: &MaterializeSpec,
    path: &Path,
) -> Result<MiniQuipu> {
    let home = std::cell::RefCell::new(home);
    materialize(
        path,
        spec,
        |request| super::record_on_legacy(catalog, &mut home.borrow_mut(), request),
        |dest, entry| fill(&home.borrow(), dest, entry),
    )
}

impl super::GraphSetOwner {
    /// Materialize a mini-Quipu of this owner's whole set: the registry entry
    /// is a control write through this owner's writer, the copy a read.
    pub fn materialize_mini_quipu(&self, spec: &MaterializeSpec, path: &Path) -> Result<MiniQuipu> {
        materialize(
            path,
            spec,
            |request| self.record_mini_fork(request),
            |dest, entry| self.owner.read(|home| fill(home, dest, entry)),
        )
    }
}

impl MiniQuipu {
    /// Open a materialized file against its home registry `entry`. A file
    /// without the identity row (any home file, or a byte copy of one) or
    /// whose minted identity differs from the entry is not a mini-Quipu.
    pub fn open(path: &Path, entry: &MiniForkEntry) -> Result<Self> {
        let path = path
            .to_str()
            .ok_or_else(|| invalid("the mini-Quipu path is not UTF-8"))?;
        let store = Store::open(path)?;
        let own = store.store_id()?;
        let recorded: Option<String> = if super::has_table(&store, "mini_quipu_identity")? {
            store
                .conn
                .query_row(
                    "SELECT store_id FROM mini_quipu_identity WHERE id = 1",
                    [],
                    |r| r.get(0),
                )
                .ok()
        } else {
            None
        };
        if own == entry.base.store_id || recorded.as_deref() != Some(own.as_str()) {
            return Err(invalid(
                "not a mini-Quipu: store_id survives a file copy, so a copied home file \
                 keeps the home identity and carries no minted identity",
            ));
        }
        if own != entry.store_id || store.log_epoch()? != entry.log_epoch {
            return Err(invalid(
                "the file's identity or epoch differs from the registry entry",
            ));
        }
        Ok(Self {
            store,
            entry: entry.clone(),
        })
    }

    /// The registry entry this file was materialized under.
    pub fn entry(&self) -> &MiniForkEntry {
        &self.entry
    }

    /// Read access to the local store.
    pub fn store(&self) -> &Store {
        &self.store
    }

    /// The local head as a cursor naming this store.
    pub fn head(&self) -> Result<StoreCursor> {
        StoreCursor::head(&self.store)
    }

    /// Resolve a cursor or `--at` position locally; a home cursor refuses X8.
    pub fn at(&self, cursor: &StoreCursor) -> Result<crate::store::AsOf> {
        cursor.admit(&self.store)
    }

    /// One local write through the ordinary write pipeline (authority,
    /// fence, SHACL under the pinned shapes, policy) to a writable member.
    pub fn transact(
        &mut self,
        graph: &str,
        datums: &[Datum],
        timestamp: &str,
        actor: Option<&str>,
    ) -> Result<i64> {
        if super::retire::fenced(&self.store)?.is_some() {
            return Err(Error::PolicyDenied(
                "mini-quipu write refused (retired-writer-fenced): the fork is retired".into(),
            ));
        }
        if self.entry.references.iter().any(|r| r.iri == graph) {
            return Err(refused(
                LocalRefusal::X6ReferenceWrite,
                "reference members are pinned copies; change them at home",
            ));
        }
        if !self.entry.writable.iter().any(|w| w == graph) {
            return Err(invalid(
                "the graph is not a writable member of this mini-Quipu",
            ));
        }
        let g = graph_id(&self.store, graph)?;
        self.store
            .transact_to_graph(datums, timestamp, actor, Some("mini-quipu"), g)
    }
}

#[cfg(test)]
#[path = "mini_quipu_local_tests.rs"]
mod tests;
