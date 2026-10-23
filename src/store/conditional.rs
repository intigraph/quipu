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

/// Complete SET slot snapshot, compared under the normal writer reservation.
pub(crate) struct SetExpectation {
    pub entity: i64,
    pub attribute: i64,
    pub values: Vec<Vec<u8>>,
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
            None,
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
            datums, timestamp, actor, source, graph, None, expected, None,
        )
    }

    pub(super) fn check_expected(
        &mut self,
        graph: i64,
        expected: &[ExpectedValue],
        datums: &[Datum],
        set: Option<&SetExpectation>,
    ) -> Result<()> {
        if expected.is_empty() && set.is_none() {
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
        if let Some(slot) = set {
            let mut query = self.conn.prepare(
                "SELECT DISTINCT v FROM facts WHERE g=?1 AND e=?2 AND a=?3 AND op=1 AND valid_to IS NULL ORDER BY v",
            )?;
            let actual = query
                .query_map(rusqlite::params![graph, slot.entity, slot.attribute], |r| {
                    r.get::<_, Vec<u8>>(0)
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            if actual != slot.values {
                return Err(Error::ConditionalWriteConflict);
            }
        }
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
        if set.is_some() && !self.shacl_config.validate_on_write && self.write_shapes.is_none() {
            return Ok(());
        }
        self.validate_conditional_datums(graph, datums, set)
    }

    fn validate_conditional_datums(
        &mut self,
        graph: i64,
        datums: &[Datum],
        set: Option<&SetExpectation>,
    ) -> Result<()> {
        let preparing = crate::time::Stopwatch::start();
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
        #[cfg(feature = "shacl")]
        let turtle = if let Some(slot) = set {
            // SET describes this subject although its request omits rdf:type.
            // Add current types before the first verdict: repair-only context
            // cannot introduce an omitted validation target later.
            let subjects = std::collections::BTreeSet::from([self.resolve(slot.entity)?]);
            let context =
                crate::shacl_context::store_type_context_in_graph(self, &subjects, graph)?;
            format!("{turtle}\n{context}")
        } else {
            turtle
        };
        crate::vocabulary::enforce_turtle(self, &turtle)
            .map_err(|e| self.stash_refusal("shacl", e, datums.len()))?;
        self.poststate_profile.conditional_prepare += preparing.elapsed();
        let loaded = if set.is_none() || self.shacl_config.validate_on_write {
            self.get_combined_shapes()?
        } else {
            None
        };
        let shapes = match (loaded, set.and(self.write_shapes.clone())) {
            (Some(loaded), Some(explicit)) => Some(format!("{loaded}\n{explicit}")),
            (loaded, explicit) => loaded.or(explicit),
        };
        if let Some(shapes) = shapes {
            #[cfg(feature = "shacl")]
            {
                let rejecting = crate::time::Stopwatch::start();
                let split = crate::shacl::split_shapes_by_policy(&shapes);
                let feedback = crate::shacl_context::validate_with_store_context_in_graph(
                    self,
                    &split.reject,
                    &turtle,
                    graph,
                )?;
                self.poststate_profile.conditional_reject += rejecting.elapsed();
                if !feedback.conforms {
                    let error = Error::ValidationFailed {
                        violations: feedback.violations,
                        messages: vec!["conditional write violates loaded shapes".into()],
                    };
                    return Err(self.stash_refusal("shacl", error, datums.len()));
                }
                if split.has_emit {
                    let emitting = crate::time::Stopwatch::start();
                    let observed = crate::shacl_context::validate_with_store_context_in_graph(
                        self,
                        &split.emit,
                        &turtle,
                        graph,
                    )?;
                    crate::shacl_context::queue_emit_violations(self, &observed);
                    self.poststate_profile.conditional_emit += emitting.elapsed();
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::promotion_control::{PromotionControl, scoped};
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    use std::time::Duration;

    #[test]
    fn conditional_conflict_and_staged_cancellation_preserve_commit_boundary() {
        let mut store = Store::open_in_memory().unwrap();
        let entity = store.intern("http://ex/conditional-cancel").unwrap();
        let attribute = store
            .intern("http://www.w3.org/2000/01/rdf-schema#label")
            .unwrap();
        let datums = [Datum {
            entity,
            attribute,
            value: Value::Str("accepted".into()),
            valid_from: "2026-01-01T00:00:00Z".into(),
            valid_to: None,
            op: crate::Op::Assert,
        }];
        let expected = [ExpectedValue {
            entity,
            attribute,
            value: None,
        }];
        let stale = [ExpectedValue {
            value: Some(Value::Str("not-current".into())),
            ..expected[0].clone()
        }];
        let rejected = PromotionControl::new(Duration::from_secs(30));
        let error = scoped(rejected.clone(), || {
            store.transact_to_graph_if(
                &datums,
                "2026-01-01T00:00:00Z",
                None,
                Some("snapshot:conditional-cancel"),
                0,
                &stale,
            )
        })
        .unwrap_err();
        assert!(matches!(error, Error::ConditionalWriteConflict));
        assert!(rejected.cancel(), "stale CAS must not claim commit");
        let before = store.current_fact_count(0).unwrap();
        let tx_before = store.latest_tx_id().unwrap();

        let control = PromotionControl::new(Duration::from_secs(30));
        let cancelled = control.clone();
        let writes = Arc::new(AtomicUsize::new(0));
        let observed = writes.clone();
        store
            .conn
            .create_scalar_function(
                "test_cancel_conditional",
                0,
                rusqlite::functions::FunctionFlags::SQLITE_UTF8,
                move |_| {
                    observed.fetch_add(1, Ordering::SeqCst);
                    cancelled.cancel();
                    Ok(0i64)
                },
            )
            .unwrap();
        store
            .conn
            .execute_batch(
                "CREATE TEMP TRIGGER test_cancel_conditional AFTER INSERT ON facts
             BEGIN SELECT test_cancel_conditional(); END;",
            )
            .unwrap();
        let error = scoped(control, || {
            store.transact_to_graph_if(
                &datums,
                "2026-01-01T00:00:00Z",
                None,
                Some("snapshot:conditional-cancel"),
                0,
                &expected,
            )
        })
        .unwrap_err();
        assert!(error.to_string().contains("cancelled"), "{error}");
        assert!(
            writes.load(Ordering::SeqCst) > 0,
            "actual SQL staging required"
        );
        assert_eq!(store.current_fact_count(0).unwrap(), before);
        assert_eq!(store.latest_tx_id().unwrap(), tx_before);
        store
            .conn
            .execute_batch("DROP TRIGGER test_cancel_conditional")
            .unwrap();

        let fresh = PromotionControl::new(Duration::from_secs(30));
        scoped(fresh.clone(), || {
            store.transact_to_graph_if(
                &datums,
                "2026-01-01T00:00:00Z",
                None,
                Some("snapshot:conditional-cancel"),
                0,
                &expected,
            )
        })
        .unwrap();
        assert_eq!(store.current_fact_count(0).unwrap(), before + 1);
        assert!(
            !fresh.cancel(),
            "successful conditional write must claim commit"
        );
    }
}
