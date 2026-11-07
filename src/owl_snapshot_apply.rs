//! Live publication and receipt checks for the private OWL snapshot.
use super::*;
use crate::write_observation::{Stage, observe, stage};

impl Snapshot {
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
        observe(Stage::Validate, || self.validate(live))?;
        let (mut job, page) = observe(Stage::Prepare, || {
            pages::check_budget(&self.scratch)?;
            let job = pages::load(&self.scratch)?;
            let page = pages::read_page(
                &self.scratch,
                "owl_page_all",
                job.apply_cursor,
                APPLY_BATCH,
                &self.timestamp,
            )?;
            Ok::<_, Error>((job, page))
        })?;
        if page.is_empty() {
            return Ok(0);
        }
        crate::write_observation::page(
            &job.generation,
            job.apply_cursor,
            page.last().expect("nonempty page").0,
            page.len(),
        );
        let preparing = stage(Stage::Remap);
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
        preparing.finish(true);
        // A lost acknowledgement leaves the local cursor untouched. The next call
        // reconciles exactly this receipt before making any additional assertion.
        observe(Stage::Transaction, || {
            live.transact_owl_page(
                &batch,
                &self.timestamp,
                companion,
                (&receipt, &digest),
                "owl:materialize",
            )
        })?;
        pages::checkpoint("after_live_commit")?;
        job.apply_cursor = end;
        job.phase = "APPLYING".into();
        observe(Stage::Acknowledge, || pages::save(&self.scratch, &job))?;
        Ok(batch.len())
    }

    pub(super) fn validate(&self, live: &Store) -> Result<()> {
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
