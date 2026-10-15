//! Current singleton-graph COUNT projection from existing covering indexes.
use super::{GraphScope, TemporalContext};
use crate::{Result, Store};

pub(super) fn projection(
    store: &Store,
    ctx: &TemporalContext,
    conditions: &[String],
) -> Result<Option<String>> {
    if store.has_attachments() {
        // Match canonical_id and Value::term_key before counting the composed
        // relation. Reference keys are integers; all literal keys are blobs,
        // so the two RDF term kinds cannot collide under SQLite GROUP BY.
        // Keep predicates on physical ids, as lookup_all already supplies every
        // dictionary id. The source retains each provider's contributed graphs.
        return Ok(Some(format!(
            "SELECT COALESCE(subject_alias.canonical_id, facts.e) AS canonical_e,
                    COALESCE(predicate_alias.canonical_id, facts.a) AS canonical_a,
                    CASE WHEN quipu_lexical_ref(facts.v) IS NULL
                         THEN quipu_term_key(facts.v)
                         ELSE COALESCE(object_alias.canonical_id,
                                       quipu_lexical_ref(facts.v)) END AS canonical_v
             FROM {}
             LEFT JOIN term_alias AS subject_alias ON subject_alias.alias_id=facts.e
             LEFT JOIN term_alias AS predicate_alias ON predicate_alias.alias_id=facts.a
             LEFT JOIN term_alias AS object_alias
                 ON object_alias.alias_id=quipu_lexical_ref(facts.v)
             WHERE {} GROUP BY canonical_e,canonical_a,canonical_v",
            store.facts_source(),
            conditions.join(" AND ")
        )));
    }
    if ctx.valid_at.is_some() || ctx.as_of_tx.is_some() {
        return Ok(None);
    }
    let (GraphScope::Default(graphs) | GraphScope::Named(graphs)) = &ctx.graph else {
        return Ok(None);
    };
    let [graph] = graphs.as_slice() else {
        return Ok(None);
    };
    let present: i64 = store.conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND tbl_name='facts'
         AND name IN ('idx_geav','idx_current_g')",
        [],
        |row| row.get(0),
    )?;
    if present != 2 {
        return Ok(None);
    }
    // Both projections refer to the SAME physical row, not just its subject.
    // The partial current-g index proves currentness; geav supplies graph and
    // terms without reading wide facts-table pages. DISTINCT is still required
    // for repeated assertions of a current triple across transactions.
    let mut predicates: Vec<String> = conditions
        .iter()
        .filter(|condition| !matches!(condition.as_str(), "op = 1" | "valid_to IS NULL"))
        .cloned()
        .collect();
    predicates.push(format!(
        "EXISTS (SELECT 1 FROM facts AS live INDEXED BY idx_current_g
         WHERE live.g={graph} AND live.rowid=facts.rowid
         AND live.op=1 AND live.valid_to IS NULL)"
    ));
    Ok(Some(format!(
        "SELECT e,a,v FROM facts INDEXED BY idx_geav WHERE {} GROUP BY e,a,quipu_term_key(v)",
        predicates.join(" AND ")
    )))
}
