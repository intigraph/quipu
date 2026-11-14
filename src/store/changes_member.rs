//! Membership conditions for change-stream filters (aegis-n0wc.3,
//! docs/design/stream-filters.md §3.1).
//!
//! A `member` condition `<predicate> <term>` admits a change record when its
//! subject has `(predicate, term)` in the record's own graph in the state just
//! BEFORE the record's transaction OR just AFTER it. The before half is the
//! deletion signal: the transaction that moves an item out of a set still
//! reaches the set's subscriber, and the next one does not.
//!
//! The term is one RDF term in N-Triples syntax matched as an EXACT term: the
//! literal is mapped to a stored [`Value`] the way RDF ingestion maps it, and
//! compared against every physical encoding of that same term
//! ([`Value::physical_aliases`]), never against a looser equivalence.
//!
//! Evaluation is one indexed read per chunk of at most [`CHUNK`] subjects of
//! one transaction, bounded by `(e, a, v)` (the fact primary key), never a
//! graph scan. The as-of predicate is the one [`Store::row_as_of`] and fork
//! snapshots use: live at `T` iff `op = 1 AND tx <= T AND (valid_to IS NULL OR
//! retracted_tx > T)`.

use std::collections::HashSet;
use std::str::FromStr;

use oxrdf::{NamedNode, Term};
use rusqlite::types::Value as Sql;
use serde_json::{Value as Json, json};

use super::{Capture, ChangePage, TermScope, id_list, page_sql, scoped_page_sql};
use crate::error::{Error, Result};
use crate::store::Store;
use crate::types::Value;

/// The longest `member` string accepted, as for the other filter strings.
pub const MAX_MEMBER_LEN: usize = 2048;
/// Subjects per membership read.
const CHUNK: usize = 500;
/// Transactions one member-filtered store page walks at most. A page that
/// walks this far without filling returns `next_tx` at the last walked
/// transaction with whatever it kept (possibly nothing): `(since, next_tx]`
/// then holds no other record in scope, and the caller pages on from there.
pub const MEMBER_WALK_TXS: usize = 256;
/// Candidate transactions fetched per walk step.
const WALK_BATCH: usize = 64;

/// Live in the state before transaction `?2`.
const BEFORE: &str = "(tx < ?2 AND (valid_to IS NULL OR retracted_tx >= ?2))";
/// Live in the state after transaction `?2`.
const AFTER: &str = "(tx <= ?2 AND (valid_to IS NULL OR retracted_tx > ?2))";

/// A parsed `member` condition, normalized: the predicate IRI and the value
/// term in canonical N-Triples (so `"a"^^xsd:string` and `"a"` are one
/// condition, and a language tag is lower-cased as RDF parsing does).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MemberSpec {
    /// The predicate IRI, without angle brackets.
    pub predicate: String,
    /// The value term in canonical N-Triples syntax.
    pub term: String,
}

impl MemberSpec {
    /// Parse `<predicate> <term>`: the predicate an `<IRI>`, the term an
    /// `<IRI>` or a quoted literal (`"x"`, `"x"@en`, `"5"^^<datatype>`).
    /// Blank nodes, quoted triples and Turtle shorthand (`5`, `true`,
    /// prefixed names) are refused.
    pub fn parse(text: &str) -> Result<Self> {
        let bad = |why: &str| {
            Error::InvalidValue(format!(
                "member must be `<predicate> <term>` in N-Triples syntax: {why}"
            ))
        };
        let text = text.trim();
        if text.len() > MAX_MEMBER_LEN {
            return Err(bad("longer than 2048 bytes"));
        }
        if !text.starts_with('<') {
            return Err(bad("the predicate is an <IRI>"));
        }
        let close = text
            .find('>')
            .ok_or_else(|| bad("unclosed predicate IRI"))?;
        let predicate = NamedNode::from_str(&text[..=close]).map_err(|e| bad(&e.to_string()))?;
        let rest = text[close + 1..].trim_start();
        if !(rest.starts_with('"') || rest.starts_with('<') && !rest.starts_with("<<")) {
            return Err(bad("the value is an <IRI> or a quoted literal"));
        }
        let term @ (Term::NamedNode(_) | Term::Literal(_)) =
            Term::from_str(rest).map_err(|e| bad(&e.to_string()))?
        else {
            return Err(bad("the value is an <IRI> or a quoted literal"));
        };
        Ok(Self {
            predicate: predicate.into_string(),
            term: term.to_string(),
        })
    }

    /// Every stored encoding of the value term, or `None` when it is an IRI
    /// the store has never seen (which no fact can hold).
    fn encodings(&self, store: &Store) -> Result<Option<Vec<Vec<u8>>>> {
        match Term::from_str(&self.term) {
            Ok(Term::NamedNode(iri)) => Ok(store
                .lookup(iri.as_str())?
                .map(|id| vec![Value::Ref(id).to_bytes()])),
            Ok(Term::Literal(literal)) => Ok(Some(
                crate::literal_identity::literal_to_value(&literal).physical_aliases(),
            )),
            _ => Err(Error::InvalidValue(format!(
                "member term is not an IRI or literal: {}",
                self.term
            ))),
        }
    }
}

/// A [`MemberSpec`] resolved against one store: the predicate's term id and
/// the value's stored encodings.
#[derive(Debug, Clone)]
pub struct MemberCondition {
    attribute: i64,
    values: Vec<Vec<u8>>,
}

impl Store {
    /// Resolve `spec`, or `None` when its predicate or IRI value is unknown
    /// to the store: such a condition holds for no subject.
    pub fn member_condition(&self, spec: &MemberSpec) -> Result<Option<MemberCondition>> {
        let Some(attribute) = self.lookup(&spec.predicate)? else {
            return Ok(None);
        };
        Ok(spec
            .encodings(self)?
            .map(|values| MemberCondition { attribute, values }))
    }

    /// Which `(graph id, entity id)` pairs satisfy `condition` in that graph
    /// in the state before transaction `tx` or after it. One read per chunk
    /// of [`CHUNK`] pairs, seeking `(e, a, v)`; returns the reads made too.
    pub fn member_pairs(
        &self,
        tx: i64,
        condition: &MemberCondition,
        pairs: &[(i64, i64)],
    ) -> Result<(HashSet<(i64, i64)>, u64)> {
        let mut unique: Vec<(i64, i64)> = pairs.to_vec();
        unique.sort_unstable();
        unique.dedup();
        let mut found = HashSet::new();
        let mut reads = 0;
        for chunk in unique.chunks(CHUNK) {
            let mut entities: Vec<i64> = chunk.iter().map(|p| p.1).collect();
            let mut graphs: Vec<i64> = chunk.iter().map(|p| p.0).collect();
            entities.sort_unstable();
            entities.dedup();
            graphs.sort_unstable();
            graphs.dedup();
            let values = (0..condition.values.len())
                .map(|i| format!("?{}", i + 3))
                .collect::<Vec<_>>()
                .join(",");
            let sql = format!(
                "SELECT DISTINCT g, e FROM facts WHERE e IN ({}) AND a = ?1 AND v IN ({values}) \
                 AND +g IN ({}) AND op = 1 AND ({BEFORE} OR {AFTER})",
                id_list(&entities),
                id_list(&graphs),
            );
            let mut bind = vec![Sql::Integer(condition.attribute), Sql::Integer(tx)];
            bind.extend(condition.values.iter().cloned().map(Sql::Blob));
            let mut stmt = self.conn.prepare(&sql)?;
            let rows = stmt.query_map(rusqlite::params_from_iter(bind), |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?))
            })?;
            reads += 1;
            let wanted: HashSet<&(i64, i64)> = chunk.iter().collect();
            for row in rows {
                let pair = row?;
                if wanted.contains(&pair) {
                    found.insert(pair);
                }
            }
        }
        Ok((found, reads))
    }

    /// The `(graph id, entity id)` of a change record, from its IRIs (both
    /// were just resolved, so these are term-cache hits). `None` for a term
    /// this store does not know, which no condition can admit.
    pub fn record_pair(&self, record: &Json) -> Result<Option<(i64, i64)>> {
        let (Some(graph), Some(entity)) = (record["graph"].as_str(), record["entity"].as_str())
        else {
            return Ok(None);
        };
        let g = if graph == "ROOT" {
            Some(0)
        } else {
            self.lookup(graph)?
        };
        Ok(match (g, self.lookup(entity)?) {
            (Some(g), Some(e)) => Some((g, e)),
            _ => None,
        })
    }

    /// [`Store::changes_after_scoped`] with a membership condition: each
    /// transaction's records in scope are kept only when their subject
    /// satisfies `member` before or after it, renumbered from 0. A
    /// transaction with none is skipped, as the stream's hub skips it. Walks
    /// at most [`MEMBER_WALK_TXS`] transactions; see there for the cursor.
    pub fn changes_after_member(
        &self,
        since_tx: i64,
        max_txs: usize,
        capture: Capture,
        graph: Option<i64>,
        scope: &TermScope,
        member: &MemberCondition,
    ) -> Result<ChangePage> {
        let (watermark_tx, watermark_timestamp) = self.latest_transaction()?;
        let sql = if scope.is_unscoped() {
            page_sql(graph.is_some()).to_string()
        } else {
            scoped_page_sql(graph.is_some(), scope)
        };
        let mut stmt = self.conn.prepare(&sql)?;
        let mut records = Vec::new();
        let (mut cursor, mut next_tx, mut kept, mut walked) = (since_tx, since_tx, 0, 0);
        'walk: loop {
            let batch: Vec<i64> = match graph {
                Some(g) => stmt
                    .query_map(rusqlite::params![cursor, WALK_BATCH as i64, g], |r| {
                        r.get(0)
                    })?
                    .collect::<std::result::Result<_, _>>()?,
                None => stmt
                    .query_map(rusqlite::params![cursor, WALK_BATCH as i64], |r| r.get(0))?
                    .collect::<std::result::Result<_, _>>()?,
            };
            if batch.is_empty() {
                // The log is walked to its end: every transaction up to the
                // last one walked was in scope and kept nothing past
                // `next_tx`, so the cursor moves past them rather than
                // re-walking them on the next wake.
                next_tx = cursor;
                break;
            }
            for tx in batch {
                cursor = tx;
                walked += 1;
                let mut candidates = Vec::new();
                let graphs = graph.as_ref().map(std::slice::from_ref);
                self.records_for_tx(tx, capture, graphs, scope, &mut candidates)?;
                let pairs: Vec<Option<(i64, i64)>> = candidates
                    .iter()
                    .map(|r| self.record_pair(r))
                    .collect::<Result<_>>()?;
                let known: Vec<(i64, i64)> = pairs.iter().flatten().copied().collect();
                let (admitted, _) = self.member_pairs(tx, member, &known)?;
                let mut sequence = 0;
                for (mut record, pair) in candidates.into_iter().zip(pairs) {
                    if pair.is_some_and(|p| admitted.contains(&p)) {
                        record["sequence"] = json!(sequence);
                        sequence += 1;
                        records.push(record);
                    }
                }
                if sequence > 0 {
                    kept += 1;
                    next_tx = tx;
                }
                if kept >= max_txs {
                    break 'walk;
                }
                if walked >= MEMBER_WALK_TXS {
                    next_tx = tx;
                    break 'walk;
                }
            }
        }
        Ok(ChangePage {
            records,
            since: since_tx,
            next_tx,
            watermark_tx,
            watermark_timestamp,
            capture,
            store_id: self.store_id()?,
            log_epoch: self.log_epoch()?,
        })
    }
}

#[cfg(test)]
#[path = "changes_member_tests.rs"]
mod tests;
