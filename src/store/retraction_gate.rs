//! The deletion gate every native retraction passes through (aegis-gxfm).
//!
//! Write-time SHACL judges what a write ASSERTS. A retraction asserts nothing,
//! so `quipu_retract`, `quipu_retract_episode` and `quipu_retract_source` (MCP,
//! REST, CLI and the Python bindings all dispatch to them) could take away a
//! value a shape requires and leave a conforming node violating `sh:minCount`,
//! accepted. `/update` already had the check (aegis-g51c C12,
//! `server/update_gates.rs`); this is the same check, moved into the store so
//! every retract path shares one definition.
//!
//! Semantics, identical to `/update`:
//!
//! - Only subjects that lose a value on a path with a floor (`sh:minCount >= 1`
//!   or `sh:hasValue`) are judged, on their full stored description before and
//!   after ([`crate::shacl_context::retraction_violations_in_graph`]). A node
//!   retracted WHOLE has no type left, so no shape targets it: removing an
//!   entity entirely stays allowed.
//! - A violation the pre-state already had is debt, not introduced: allowed.
//! - Reject-policy shapes refuse; nothing is written and a `shacl` gate refusal
//!   is recorded. Emit-policy shapes never block: their findings are queued as
//!   `shacl.violation` events that ride the retraction's own transaction.
//! - `shacl.validate_on_write = false` restores the previous behaviour exactly.
//!
//! Writes that replace rather than only retract run it too: an episode
//! `replace_snapshot` re-ingest and a share-import promotion into a target
//! graph both close facts the new payload lacks.
//!
//! There is no per-call bypass. The orphan flags (`allow_orphan`, `on_orphan`)
//! are about node identity, not shapes, and do not override this gate.

use super::{Datum, Store};
use crate::error::Result;

impl Store {
    /// Refuse a write to `graph` whose `Op::Retract` datums leave a node newly
    /// violating a reject shape's floor; its `Op::Assert` datums count toward
    /// the post-state, so a replacement value satisfies the floor. `Ok(())`
    /// means it may be transacted.
    ///
    /// `scope` is the `retract_source` the transaction will close claims under
    /// (episode, source and snapshot retraction). A statement another producer
    /// also claims stays visible after such a retraction, so it is not judged
    /// as removed.
    #[cfg_attr(not(feature = "shacl"), allow(clippy::unused_self))]
    pub(crate) fn enforce_retraction_shapes(
        &self,
        datums: &[Datum],
        graph: i64,
        scope: Option<&str>,
        actor: Option<&str>,
        source: Option<&str>,
    ) -> Result<()> {
        #[cfg(feature = "shacl")]
        {
            use crate::types::Op;
            if !self.shacl_config.validate_on_write || datums.iter().all(|d| d.op != Op::Retract) {
                return Ok(());
            }
            let (mut removed, mut asserted) = (String::new(), String::new());
            for datum in datums {
                let out = match datum.op {
                    Op::Retract => {
                        if let Some(scope) = scope
                            && self.claimed_outside(datum, graph, scope)?
                        {
                            continue;
                        }
                        &mut removed
                    }
                    Op::Assert => &mut asserted,
                    // An overlay tombstone is not a graph retraction.
                    Op::Tombstone => continue,
                };
                out.push_str(&self.datum_ntriple(datum)?);
            }
            self.enforce_retraction_shapes_nt(&removed, &asserted, graph, actor, source)
        }
        #[cfg(not(feature = "shacl"))]
        {
            let _ = (datums, graph, scope, actor, source);
            Ok(())
        }
    }

    /// One datum as an N-Triples line. Terms come from the shared
    /// value-to-term helper, so a blank-node subject or object is `_:b`, never
    /// an `<_:b>` that would not parse.
    #[cfg(feature = "shacl")]
    fn datum_ntriple(&self, datum: &Datum) -> Result<String> {
        let subject = crate::rdf::value_to_term(self, &crate::types::Value::Ref(datum.entity))?;
        let object = crate::rdf::value_to_term(self, &datum.value)?;
        let predicate = self.resolve(datum.attribute)?;
        Ok(format!("{subject} <{predicate}> {object} .\n"))
    }

    /// Does a producer other than `scope` hold a live claim on `datum` in
    /// `graph`? Then a retraction scoped to `scope` leaves it visible.
    #[cfg(feature = "shacl")]
    fn claimed_outside(&self, datum: &Datum, graph: i64, scope: &str) -> Result<bool> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT 1 FROM facts f JOIN transactions t ON f.tx = t.id \
             WHERE f.e = ?1 AND f.a = ?2 AND f.v = ?3 AND f.g = ?4 \
               AND f.op = 1 AND f.valid_to IS NULL \
               AND (t.source IS NULL OR t.source <> ?5) \
             LIMIT 1",
        )?;
        for bytes in self.literal_aliases(&datum.value, false)? {
            if stmt.exists(rusqlite::params![
                datum.entity,
                datum.attribute,
                bytes,
                graph,
                scope
            ])? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// [`Store::enforce_retraction_shapes`] over N-Triples: `removed_nt` leaves
    /// graph `g` while `asserted_nt` lands there in the same write (`/update`'s
    /// DELETE/INSERT halves, a merge's removals and additions). On a refusal a
    /// `shacl` gate refusal is recorded and nothing should be written.
    #[cfg_attr(not(feature = "shacl"), allow(clippy::unused_self))]
    pub fn enforce_retraction_shapes_nt(
        &self,
        removed_nt: &str,
        asserted_nt: &str,
        g: i64,
        actor: Option<&str>,
        source: Option<&str>,
    ) -> Result<()> {
        #[cfg(feature = "shacl")]
        if let Some(feedback) = self.retraction_refusal_nt(removed_nt, asserted_nt, g, true)? {
            let reason = format!(
                "SHACL validation failed: {} violation(s) introduced by a deletion: {}. \
                 No facts were written",
                feedback.violations,
                refusal_detail(&feedback)
            );
            let now = crate::time::now_iso();
            self.record_gate_refusal("shacl", &reason, g, actor, source, &now);
            return Err(crate::error::Error::InvalidValue(reason));
        }
        #[cfg(not(feature = "shacl"))]
        let _ = (removed_nt, asserted_nt, g, actor, source);
        Ok(())
    }

    /// The reject-shape violations `removed_nt` would introduce in graph `g`,
    /// or `None` when it may be written (also when `validate_on_write` is off
    /// or no shapes are stored). With `queue_emit`, emit-shape findings are
    /// queued as `shacl.violation` events; a dry run passes `false`.
    #[cfg(feature = "shacl")]
    pub fn retraction_refusal_nt(
        &self,
        removed_nt: &str,
        asserted_nt: &str,
        g: i64,
        queue_emit: bool,
    ) -> Result<Option<crate::shacl::ValidationFeedback>> {
        if removed_nt.is_empty() || !self.shacl_config.validate_on_write {
            return Ok(None);
        }
        let Some(stored) = self.get_combined_shapes()? else {
            return Ok(None);
        };
        let split = crate::shacl::split_shapes_by_policy(&stored);
        let feedback = crate::shacl_context::retraction_violations_in_graph(
            self,
            &split.reject,
            removed_nt,
            asserted_nt,
            g,
        )?;
        if !feedback.conforms {
            return Ok(Some(feedback));
        }
        if queue_emit && split.has_emit {
            let observed = crate::shacl_context::retraction_violations_in_graph(
                self,
                &split.emit,
                removed_nt,
                asserted_nt,
                g,
            )?;
            crate::shacl_context::queue_emit_violations(self, &observed);
        }
        Ok(None)
    }
}

/// The first three issues, for a refusal message.
#[cfg(feature = "shacl")]
fn refusal_detail(feedback: &crate::shacl::ValidationFeedback) -> String {
    feedback
        .results
        .iter()
        .take(3)
        .map(|r| {
            format!(
                "{}: {} [{}] ({})",
                r.severity,
                r.message.as_deref().unwrap_or("no message"),
                r.source_shape.as_deref().unwrap_or("?"),
                r.focus_node
            )
        })
        .collect::<Vec<_>>()
        .join("; ")
}

#[cfg(all(test, feature = "shacl"))]
#[path = "retraction_gate_tests.rs"]
mod tests;
