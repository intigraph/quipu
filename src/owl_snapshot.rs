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
            scratch.load_ontology(name, turtle, loaded_at)?;
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

    /// Publish freshness only after every batch succeeded and its premises are
    /// still valid. Uses indexed point reads, never a companion-wide scan.
    pub fn finish(&self, live: &mut Store) -> Result<()> {
        pages::check_budget(&self.scratch)?;
        if !self.derived || self.remaining() != 0 {
            return Err(Error::InvalidValue("OWL snapshot is incomplete".into()));
        }
        self.validate(live)?;
        let job = pages::load(&self.scratch)?;
        let (count, last): (i64, i64) = self.scratch.conn.query_row(
            "SELECT count(*),coalesce(max(id),0) FROM owl_page_all",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let receipts: i64 = live.conn.query_row(
            "SELECT count(*) FROM owl_batch_receipts WHERE batch GLOB ?1 AND batch<>?2",
            params![
                format!("{}:*", job.generation),
                format!("{}:finish", job.generation)
            ],
            |r| r.get(0),
        )?;
        let count = pages::cardinality(count)?;
        let receipts = pages::cardinality(receipts)?;
        if count != job.report.total
            || job.apply_cursor != last
            || receipts != count.div_ceil(APPLY_BATCH)
        {
            return Err(Error::InvalidValue(
                "OWL publication receipts/cursor do not cover the complete closure".into(),
            ));
        }
        if self.ontologies.is_empty() {
            let mut job = pages::load(&self.scratch)?;
            job.phase = "COMPLETE".into();
            pages::save(&self.scratch, &job)?;
            return Ok(());
        }
        let graph = live.intern(ROOT_INFERRED_GRAPH_IRI)?;
        let attr = live.intern(DERIVED_AS_OF_TX)?;
        let mut datums = Vec::new();
        {
            let mut stmt = live.conn.prepare(
                "SELECT v FROM facts WHERE g=?1 AND e=?1 AND a=?2 AND op=1 AND valid_to IS NULL",
            )?;
            let mut rows = stmt.query(params![graph, attr])?;
            while let Some(row) = rows.next()? {
                datums.push(Datum {
                    entity: graph,
                    attribute: attr,
                    value: Value::from_bytes(&row.get::<_, Vec<u8>>(0)?)?,
                    valid_from: self.timestamp.clone(),
                    valid_to: None,
                    op: Op::Retract,
                });
            }
        }
        datums.push(Datum {
            entity: graph,
            attribute: attr,
            value: Value::Int(self.premise_head),
            valid_from: self.timestamp.clone(),
            valid_to: None,
            op: Op::Assert,
        });
        datums.push(Datum {
            entity: graph,
            attribute: live.intern(SOURCE_KIND)?,
            value: Value::Str("inferred".into()),
            valid_from: self.timestamp.clone(),
            valid_to: None,
            op: Op::Assert,
        });
        let job = pages::load(&self.scratch)?;
        let receipt = format!("{}:finish", job.generation);
        let digest = format!("{}:{}:{}", job.identity, job.premise_head, job.generation);
        live.transact_owl_page(
            &datums,
            &self.timestamp,
            graph,
            (&receipt, &digest),
            PLANE_SOURCE,
        )?;
        pages::checkpoint("after_freshness_commit")?;
        let mut job = pages::load(&self.scratch)?;
        job.phase = "COMPLETE".into();
        pages::save(&self.scratch, &job)?;
        Ok(())
    }

    /// Validate monotone evolution, then apply at most [`APPLY_BATCH`] assertions.
    /// Concurrent additions are allowed: the result is explicitly as-of the
    /// snapshot, not a claim that new facts have been processed. Retractions or
    /// ontology changes stop publication. Already committed batches are partial
    /// historical entailments; callers must report failure and not freshness.
    pub fn apply_batch(&mut self, live: &mut Store) -> Result<usize> {
        if !self.derived {
            return Err(Error::InvalidValue(
                "OWL snapshot has not been derived".into(),
            ));
        }
        self.validate(live)?;
        pages::check_budget(&self.scratch)?;
        let mut job = pages::load(&self.scratch)?;
        let page = pages::read_page(
            &self.scratch,
            "owl_page_all",
            job.apply_cursor,
            APPLY_BATCH,
            &self.timestamp,
        )?;
        if page.is_empty() {
            return Ok(0);
        }
        let mut batch = Vec::with_capacity(page.len());
        let mut digest = sha2::Sha256::new();
        use sha2::Digest;
        // Digest semantic values, never transient live term ids. Length-prefix every field.
        for (_, datum) in &page {
            let entity = self.scratch.resolve(datum.entity)?;
            let attribute = self.scratch.resolve(datum.attribute)?;
            let value = match &datum.value {
                Value::Ref(id) => format!("ref:{}", self.scratch.resolve(*id)?).into_bytes(),
                value => value.to_bytes(),
            };
            for field in [entity.as_bytes(), attribute.as_bytes(), value.as_slice()] {
                digest.update((field.len() as u64).to_le_bytes());
                digest.update(field);
            }
            batch.push(Datum {
                entity: remap_id(&self.scratch, live, datum.entity)?,
                attribute: remap_id(&self.scratch, live, datum.attribute)?,
                value: remap_value(&self.scratch, live, datum.value.clone())?,
                valid_from: self.timestamp.clone(),
                valid_to: None,
                op: Op::Assert,
            });
        }
        let end = page.last().expect("nonempty page").0;
        let receipt = format!("{}:{}:{}", job.generation, job.apply_cursor, end);
        let digest = format!("{:x}", digest.finalize());
        let companion = live.intern(ROOT_INFERRED_GRAPH_IRI)?;
        // A lost acknowledgement leaves the local cursor untouched. The next call
        // reconciles exactly this receipt before making any additional assertion.
        live.transact_owl_page(
            &batch,
            &self.timestamp,
            companion,
            (&receipt, &digest),
            "owl:materialize",
        )?;
        pages::checkpoint("after_live_commit")?;
        job.apply_cursor = end;
        job.phase = "APPLYING".into();
        pages::save(&self.scratch, &job)?;
        Ok(batch.len())
    }

    fn validate(&self, live: &Store) -> Result<()> {
        let job = pages::load(&self.scratch)?;
        if matches!(job.phase.as_str(), "FAILED" | "INVALIDATED") {
            return Err(Error::InvalidValue(format!(
                "OWL job {}: {}",
                job.phase,
                job.failure.unwrap_or_default()
            )));
        }
        let result = self.validate_inner(live);
        if let Err(Error::InvalidValue(reason)) = &result
            && let Ok(mut job) = pages::load(&self.scratch)
        {
            job.phase = "INVALIDATED".into();
            job.failure = Some(reason.clone());
            let _ = pages::save(&self.scratch, &job);
        }
        result
    }

    fn validate_inner(&self, live: &Store) -> Result<()> {
        if live.store_id()? != self.identity || live.list_ontologies()? != self.ontologies {
            return Err(Error::InvalidValue(
                "OWL snapshot identity or ontology changed".into(),
            ));
        }
        let freshness = live.lookup(DERIVED_AS_OF_TX)?.unwrap_or(-1);
        for graph in std::iter::once(0).chain(live.lookup(ROOT_INFERRED_GRAPH_IRI)?) {
            let retracted: bool = live.conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM facts INDEXED BY idx_retracted_tx \
                 WHERE g=?1 AND retracted_tx>?2 \
                 AND NOT (g!=0 AND e=?1 AND a=?3))",
                params![graph, self.premise_head, freshness],
                |row| row.get(0),
            )?;
            if retracted {
                return Err(Error::InvalidValue(
                    "OWL snapshot premises retracted; retry required".into(),
                ));
            }
        }
        Ok(())
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
