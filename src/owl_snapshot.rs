//! Scheduled derivation over a private, current-state snapshot.
//!
//! Snapshot construction and derivation must run on a read connection, outside
//! both the live writer mutex and write admission. History, vectors and unrelated
//! named graphs are deliberately not copied. Applying a proposal is separate so
//! callers can release writer admission between bounded batches.

use rusqlite::params;

use crate::error::{Error, Result};
use crate::store::inferred::{
    DERIVED_AS_OF_TX, PLANE_SOURCE, ROOT_INFERRED_GRAPH_IRI, SOURCE_KIND,
};
use crate::{Datum, MaterializeReport, Op, Store, Value};

#[path = "owl_snapshot_apply.rs"]
mod apply;
#[path = "owl_snapshot_footprint.rs"]
mod footprint;
#[path = "owl_snapshot_pages.rs"]
pub(crate) mod pages;
#[path = "owl_snapshot_persistent.rs"]
mod persistent;

/// Maximum number of proposed assertions applied by one writer acquisition.
pub const APPLY_BATCH: usize = 64;
/// Refuse oversized inputs instead of copying the entire historical database.
const MAX_PREMISES: usize = 3_000_000;
const MAX_PROPOSALS: usize = 200_000;

/// Private derivation state. Its term ids are never used directly in the live store.
pub struct Snapshot {
    scratch: Store,
    identity: String,
    ontologies: Vec<(String, String, String)>,
    source_companion: Option<i64>,
    /// Transaction watermark of the consistent premise snapshot.
    pub premise_head: i64,
    timestamp: String,
    derived: bool,
    /// Derivation counts, before applying to the live store.
    pub report: MaterializeReport,
}

impl Snapshot {
    /// Private temporary SQLite store, deleted when its connection closes.
    /// SQLite's empty filename creates an anonymous disk-backed database rather
    /// than retaining the snapshot in RAM. Cache limits concern this private
    /// store only; derivation still allocates premise and proposal vectors.
    pub fn temporary_store() -> Result<Store> {
        let scratch = Store::open("")?;
        scratch.conn.execute_batch(
            "PRAGMA cache_size=-8192; PRAGMA mmap_size=0; PRAGMA temp_store=FILE;",
        )?;
        scratch.set_term_cache_limit(32_768);
        Ok(scratch)
    }

    /// Copy the complete OWL premise footprint from current ROOT and its inferred
    /// companion under one consistent read transaction. Unrelated facts are
    /// omitted only when the ontology/identity rules prove them irrelevant;
    /// identity-producing property axioms retain the full-input path and cap.
    /// `scratch` must be empty and private. No live writes or derivation occur here.
    pub fn capture(source: &Store, scratch: Store, timestamp: &str) -> Result<Self> {
        Self::capture_with_limit(source, scratch, timestamp, MAX_PREMISES)
    }

    fn capture_with_limit(
        source: &Store,
        scratch: Store,
        timestamp: &str,
        max_premises: usize,
    ) -> Result<Self> {
        pages::limits(&scratch)?;
        if !scratch.current_facts()?.is_empty() || !scratch.list_ontologies()?.is_empty() {
            return Err(Error::InvalidValue(
                "OWL scratch store must be empty".into(),
            ));
        }
        let read = source.conn.unchecked_transaction()?;
        let premise_head = source.transaction_head()?;
        let identity = source.store_id()?;
        let ontologies = source.list_ontologies()?;
        let source_companion = source.lookup(ROOT_INFERRED_GRAPH_IRI)?;
        let footprint =
            footprint::Footprint::discover(source, &ontologies, source_companion, max_premises)?;
        let companion = scratch.intern(ROOT_INFERRED_GRAPH_IRI)?;
        let write = scratch.conn.unchecked_transaction()?;
        scratch.conn.execute(
            "INSERT INTO transactions(timestamp, source) VALUES (?1, 'owl:snapshot')",
            params![timestamp],
        )?;
        let tx = scratch.conn.last_insert_rowid();
        let mut count = 0;
        for graph in std::iter::once(0).chain(source_companion) {
            footprint.for_each(source, graph, |row| {
                count += 1;
                if count > max_premises {
                    return Err(Error::InvalidValue(
                        "OWL snapshot premise budget exceeded".into(),
                    ));
                }
                let entity = remap_id(source, &scratch, row.get(0)?)?;
                let attribute = remap_id(source, &scratch, row.get(1)?)?;
                let value = remap_value(
                    source,
                    &scratch,
                    Value::from_bytes(&row.get::<_, Vec<u8>>(2)?)?,
                )?;
                scratch.conn.execute(
                    "INSERT OR IGNORE INTO facts(e,a,v,g,tx,valid_from,op) \
                     VALUES (?1,?2,?3,?4,?5,?6,1)",
                    params![
                        entity,
                        attribute,
                        value.to_bytes(),
                        if graph == 0 { 0 } else { companion },
                        tx,
                        row.get::<_, String>(3)?
                    ],
                )?;
                Ok(())
            })?;
        }
        for (name, turtle, loaded_at) in &ontologies {
            scratch.load_snapshot_ontology(name, turtle, loaded_at)?;
        }
        let committed = write.commit();
        crate::store::release::settle(&scratch.conn, committed)?;
        read.commit()?;
        let snapshot = Self {
            scratch,
            identity,
            ontologies,
            source_companion,
            premise_head,
            timestamp: timestamp.to_owned(),
            derived: false,
            report: MaterializeReport::default(),
        };
        pages::initialize(&snapshot)?;
        Ok(snapshot)
    }

    /// Compute the closure on the private store. No live lock is needed.
    pub fn derive(&mut self) -> Result<()> {
        if self.derived {
            return Err(Error::InvalidValue("OWL snapshot already derived".into()));
        }
        pages::derive(self, MAX_PROPOSALS)
    }

    /// Reopen a private snapshot without renewing its deadline or logical-pass budget.
    /// The caller supplies the same private database and ordinary resource admission.
    pub fn resume(scratch: Store) -> Result<Self> {
        pages::limits(&scratch)?;
        let job = pages::load(&scratch)?;
        let derived = matches!(
            job.phase.as_str(),
            "FIXPOINT_COMPLETE" | "APPLYING" | "COMPLETE"
        );
        Ok(Self {
            scratch,
            identity: job.identity,
            ontologies: job.ontologies,
            source_companion: job.source_companion,
            premise_head: job.premise_head,
            timestamp: job.timestamp,
            derived,
            report: job.report,
        })
    }

    /// Number of ontologies in the snapshot (zero is distinct from an empty closure).
    pub fn ontology_count(&self) -> usize {
        self.ontologies.len()
    }

    /// Number of proposals remaining to apply.
    pub fn remaining(&self) -> usize {
        // A missing/unreadable queue must never appear complete.
        let result = (|| -> Result<usize> {
            let job = pages::load(&self.scratch)?;
            let count = self.scratch.conn.query_row(
                "SELECT count(*) FROM owl_page_all WHERE id>?1",
                [job.apply_cursor],
                |r| r.get::<_, i64>(0),
            )?;
            pages::cardinality(count)
        })();
        result.unwrap_or(usize::MAX)
    }
}

#[cfg(test)]
#[path = "owl_snapshot_pages_tests.rs"]
mod pages_tests;

#[cfg(test)]
#[path = "owl_snapshot_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "owl_snapshot_footprint_tests.rs"]
mod footprint_tests;

fn remap_id(source: &Store, target: &Store, id: i64) -> Result<i64> {
    target.intern(&source.resolve(id)?)
}

fn remap_value(source: &Store, target: &Store, value: Value) -> Result<Value> {
    match value {
        Value::Ref(id) => Ok(Value::Ref(remap_id(source, target, id)?)),
        value => Ok(value),
    }
}
