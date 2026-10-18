//! Compare current values inside the normal write savepoint, not an outer
//! transaction which would roll back durable denial verdicts/refusal events.

use super::{Datum, Store};
use crate::{Error, Result, Value};

/// A complete single-valued slot expectation. None means no asserted value.
#[derive(Clone, Debug)]
pub struct ExpectedValue {
    pub entity: i64,
    pub attribute: i64,
    pub value: Option<Value>,
}

impl Store {
    /// Write a batch to a specific named graph. g=0 is ROOT. Idempotency and
    /// retraction are SCOPED to `graph`: writing to an overlay never skips a
    /// triple because ROOT has it, and never closes a ROOT fact's `valid_to`. The
    /// same (e,a,v) coexists across graphs, and an overlay's retract closes only
    /// its OWN assertions — the base stays un-mutated (Stiwi's #36 requirement,
    /// enforced in SQL, not convention). Cross-graph SHADOWING (an overlay
    /// hiding a ROOT fact in the composed view) is a read-time resolution over
    /// the ordered dataset stack, handled by the query path — not a base write.
    pub fn transact_to_graph(
        &mut self,
        datums: &[Datum],
        timestamp: &str,
        actor: Option<&str>,
        source: Option<&str>,
        graph: i64,
    ) -> Result<i64> {
        self.transact_to_graph_scoped(datums, timestamp, actor, source, graph, None)
    }

    /// Source cleanup must preserve assertions belonging to other producers.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn transact_to_graph_scoped(
        &mut self,
        datums: &[Datum],
        timestamp: &str,
        actor: Option<&str>,
        source: Option<&str>,
        graph: i64,
        retract_source: Option<&str>,
    ) -> Result<i64> {
        self.transact_to_graph_scoped_checked(
            datums,
            timestamp,
            actor,
            source,
            graph,
            retract_source,
            &[],
        )
    }

    /// Conditional graph write with the same authority/SHACL/policy pipeline.
    #[allow(clippy::too_many_arguments)]
    pub fn transact_to_graph_if(
        &mut self,
        datums: &[Datum],
        timestamp: &str,
        actor: Option<&str>,
        source: Option<&str>,
        graph: i64,
        expected: &[ExpectedValue],
    ) -> Result<i64> {
        self.transact_to_graph_scoped_checked(
            datums, timestamp, actor, source, graph, None, expected,
        )
    }

    pub(super) fn check_expected(
        &mut self,
        graph: i64,
        expected: &[ExpectedValue],
        datums: &[Datum],
    ) -> Result<()> {
        if expected.is_empty() {
            return Ok(());
        }
        if expected.len() > 16 {
            return Err(Error::InvalidValue("too many write preconditions".into()));
        }
        // Acquire SQLite's writer reservation BEFORE reading heads. This
        // zero-row DML changes no facts/transactions but excludes other writers
        // until the enclosing normal write savepoint releases/rolls back.
        self.conn
            .execute("UPDATE transactions SET id=id WHERE 0", [])?;
        let mut statement = self.conn.prepare(
            "SELECT DISTINCT v FROM facts WHERE g=?1 AND e=?2 AND a=?3 \
             AND op=1 AND valid_to IS NULL LIMIT 2",
        )?;
        for slot in expected {
            let bytes = statement
                .query_map(
                    rusqlite::params![graph, slot.entity, slot.attribute],
                    |row| row.get::<_, Vec<u8>>(0),
                )?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            let actual = bytes
                .iter()
                .map(|v| Value::from_bytes(v))
                .collect::<Result<Vec<_>>>()?;
            let matches = match (&slot.value, actual.as_slice()) {
                (None, []) => true,
                (Some(expected), [actual]) => expected == actual,
                _ => false,
            };
            if !matches {
                return Err(Error::ConditionalWriteConflict);
            }
        }
        drop(statement);
        // Store's low-level datum pipeline handles authority/OWL/policy, while
        // ordinary RDF entrypoints add vocabulary/SHACL admission themselves.
        // Conditional file writes must not silently skip that RDF admission.
        self.validate_conditional_datums(graph, datums)
    }

    fn validate_conditional_datums(&mut self, graph: i64, datums: &[Datum]) -> Result<()> {
        let assertions = datums
            .iter()
            .filter(|d| d.op == crate::Op::Assert)
            .map(|d| crate::Fact {
                entity: d.entity,
                attribute: d.attribute,
                value: d.value.clone(),
                tx: 0,
                valid_from: d.valid_from.clone(),
                valid_to: d.valid_to.clone(),
                op: d.op,
            })
            .collect::<Vec<_>>();
        let bytes = crate::rdf::serialize_facts(self, &assertions, oxrdfio::RdfFormat::NTriples)?;
        let turtle = String::from_utf8(bytes)
            .map_err(|_| Error::InvalidValue("invalid conditional RDF".into()))?;
        crate::vocabulary::enforce_turtle(self, &turtle)
            .map_err(|e| self.stash_refusal("shacl", e, datums.len()))?;
        if let Some(shapes) = self.get_combined_shapes()? {
            #[cfg(feature = "shacl")]
            {
                let split = crate::shacl::split_shapes_by_policy(&shapes);
                let feedback = crate::shacl_context::validate_with_store_context_in_graph(
                    self,
                    &split.reject,
                    &turtle,
                    graph,
                )?;
                if !feedback.conforms {
                    let error = Error::ValidationFailed {
                        violations: feedback.violations,
                        messages: vec!["conditional write violates loaded shapes".into()],
                    };
                    return Err(self.stash_refusal("shacl", error, datums.len()));
                }
                if split.has_emit {
                    let observed = crate::shacl_context::validate_with_store_context_in_graph(
                        self,
                        &split.emit,
                        &turtle,
                        graph,
                    )?;
                    crate::shacl_context::queue_emit_violations(self, &observed);
                }
            }
            #[cfg(not(feature = "shacl"))]
            {
                let _ = (shapes, graph);
                return Err(Error::CannotVerify(
                    "conditional write requires SHACL support for loaded shapes".into(),
                ));
            }
        }
        Ok(())
    }
}
