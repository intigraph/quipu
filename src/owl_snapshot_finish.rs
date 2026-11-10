//! Complete cumulative coverage is prepared privately, then checked live.
use super::prepare::{Binding, invalid};
use super::*;
use crate::write_observation::{Stage, observe};
use std::collections::BTreeMap;

/// Full frozen cumulative receipt manifest, prepared without the live mutex.
pub struct PreparedFinish {
    binding: Binding,
    receipts: BTreeMap<String, String>,
    cursor: i64,
    count: usize,
}
/// Successful freshness publication (or the explicit no-ontology case).
pub struct CommittedFinish {
    prepared: PreparedFinish,
}

impl Snapshot {
    /// Prepare exact contiguous page/digest coverage outside writer admission.
    pub fn prepare_finish(&self) -> Result<PreparedFinish> {
        observe(Stage::Prepare, || {
            pages::check_budget(&self.scratch)?;
            if !self.derived || self.remaining() != 0 {
                return Err(invalid("OWL snapshot is incomplete"));
            }
            let job = pages::load(&self.scratch)?;
            let binding = Binding::capture(self, &job)?;
            let mut receipts = BTreeMap::new();
            let (mut cursor, mut count) = (0, 0);
            while let Some(page) = super::prepare::prepare_page(self, &job, cursor)? {
                pages::check_deadline(binding.deadline)?;
                if page.end() <= cursor {
                    return Err(invalid("OWL cumulative cursor did not advance"));
                }
                cursor = page.end();
                count += page.len();
                if receipts
                    .insert(page.receipt(), page.digest().to_owned())
                    .is_some()
                {
                    return Err(invalid("OWL duplicate cumulative receipt"));
                }
            }
            if count != job.report.total || job.apply_cursor != cursor {
                return Err(invalid(
                    "OWL publication receipts/cursor do not cover the complete closure",
                ));
            }
            Ok(PreparedFinish {
                binding,
                receipts,
                cursor,
                count,
            })
        })
    }
    /// Persist COMPLETE only after a checked successful finish receipt.
    pub fn reconcile_finish(&self, committed: Result<CommittedFinish>) -> Result<()> {
        let committed = committed.inspect_err(|error| self.record_invalidation(error))?;
        observe(Stage::Acknowledge, || {
            pages::checkpoint("after_freshness_commit")?;
            let prepared = committed.prepared;
            let mut job = pages::load(&self.scratch)?;
            if Binding::capture(self, &job)? != prepared.binding
                || job.apply_cursor != prepared.cursor
                || job.report.total != prepared.count
            {
                return Err(invalid(
                    "OWL finish acknowledgement generation/cursor changed",
                ));
            }
            job.phase = "COMPLETE".into();
            pages::save(&self.scratch, &job)
        })
    }
}

impl PreparedFinish {
    /// Verify every expected receipt, then atomically publish normal freshness.
    /// No private database access or cursor persistence is performed here.
    pub fn apply(self, live: &mut Store) -> Result<CommittedFinish> {
        observe(Stage::Validate, || {
            self.binding.validate(live)?;
            let mut statement = live.conn.prepare(
                "SELECT batch,digest FROM owl_batch_receipts WHERE batch GLOB ?1 AND batch<>?2",
            )?;
            let mut rows = statement.query(params![
                format!("{}:*", self.binding.generation),
                format!("{}:finish", self.binding.generation)
            ])?;
            let mut count = 0;
            while let Some(row) = rows.next()? {
                let name: String = row.get(0)?;
                let digest: String = row.get(1)?;
                if self.receipts.get(&name) != Some(&digest) {
                    return Err(invalid(
                        "OWL publication receipts/cursor do not cover the complete closure",
                    ));
                }
                count += 1;
            }
            if count != self.receipts.len() {
                return Err(invalid(
                    "OWL publication receipts/cursor do not cover the complete closure",
                ));
            }
            Ok(())
        })?;
        if self.binding.ontologies.is_empty() {
            return Ok(CommittedFinish { prepared: self });
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
                    valid_from: self.binding.timestamp.clone(),
                    valid_to: None,
                    op: Op::Retract,
                });
            }
        }
        datums.push(Datum {
            entity: graph,
            attribute: attr,
            value: Value::Int(self.binding.premise_head),
            valid_from: self.binding.timestamp.clone(),
            valid_to: None,
            op: Op::Assert,
        });
        datums.push(Datum {
            entity: graph,
            attribute: live.intern(SOURCE_KIND)?,
            value: Value::Str("inferred".into()),
            valid_from: self.binding.timestamp.clone(),
            valid_to: None,
            op: Op::Assert,
        });
        let receipt = format!("{}:finish", self.binding.generation);
        let digest = format!(
            "{}:{}:{}",
            self.binding.identity, self.binding.premise_head, self.binding.generation
        );
        pages::check_deadline(self.binding.deadline)?;
        observe(Stage::Transaction, || {
            live.transact_owl_page(
                &datums,
                &self.binding.timestamp,
                graph,
                (&receipt, &digest),
                PLANE_SOURCE,
            )
        })?;
        Ok(CommittedFinish { prepared: self })
    }
}
