//! Fenced reverse reconciliation (per-graph pilot, aegis-4qv.3, DESIGN v2).
//!
//! After the owner accepted writes, rolling a routed graph back to the legacy
//! file must not lose them and must never route to the stale legacy copy. The
//! procedure is offline and legacy-side only; it contacts no server and builds
//! no witness handoff (owner witness retirement is aegis-4qv.4):
//!
//! 0. The caller holds the owner file exclusively ([`lock_owner_file`]) for
//!    the whole run, legacy is fenced for the graph at exactly `(R, G)`, and
//!    the operator-published catalog `R+1` routes this graph, and only this
//!    graph, back to legacy ([`validate_reverse_catalogs`]).
//! 1. The owner's FULL named graph is exported with its source tags.
//! 2. The legacy fence moves `(R, G)` → `Reconciling(R+1, G+1)`.
//! 3. In ONE savepoint the fence admits only this importer, every owner fact
//!    is written into the same named graph under its own source tag, and the
//!    whole-graph multiset `legacy == owner` is proven before the fence row is
//!    restored. A mismatch rolls the import back and stays Reconciling.
//! 4. The fence (graph row and its producer source rows) is lifted at
//!    `(R+1, G+1)`, and only from the proven state.
//!
//! Each step is one SQLite transaction recorded in `routed_graph_reconcile`,
//! which is the intent journal: an interrupted run resumes from the recorded
//! state to the identical receipt. The owner file is only read.

use super::routed_fence::{FENCE_TABLE, SOURCE_FENCE_TABLE};
use super::{SliceFact, Store};
use crate::types::{Op, Value};
use crate::{Error, Result};
use rusqlite::{OptionalExtension, params};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

const JOURNAL: &str = "routed_graph_reconcile";
const SAVEPOINT: &str = "quipu_routed_reverse";
/// Recorded in every receipt until owner witness retirement (aegis-4qv.4).
pub const WITNESS_STATE: &str = "active_unretired";

const GRAPH_FACTS_SQL: &str = "SELECT f.e, f.a, f.v, f.valid_from, t.source \
     FROM facts f JOIN transactions t ON f.tx = t.id \
     WHERE +f.g = ?1 AND f.op = 1 AND f.valid_to IS NULL \
     ORDER BY f.e, f.a, f.v, f.valid_from, t.source";

fn refused(message: impl std::fmt::Display) -> Error {
    Error::PolicyDenied(format!("graph owner reverse: {message}"))
}

/// One current fact of a routed graph with the source tag that wrote it.
#[derive(Clone, Debug, PartialEq)]
pub struct GraphFact {
    /// The `transactions.source` tag that wrote it (`None` when untagged).
    pub source: Option<String>,
    /// The fact in canonical, file-independent form.
    pub fact: SliceFact,
}

impl GraphFact {
    fn canonical(&self, graph: &str) -> String {
        let source = self
            .source
            .as_deref()
            .map_or("-".to_string(), |s| format!("{s:?}"));
        format!("<{graph}> {} {source}", self.fact.canonical())
    }
}

/// SHA-256 over the sorted canonical `(graph, s, p, o, valid_from, source)`
/// multiset. Equal digests over the same graph mean equal fact multisets.
#[must_use]
pub fn graph_multiset_sha256(graph: &str, facts: &[GraphFact]) -> String {
    let mut lines: Vec<String> = facts.iter().map(|f| f.canonical(graph)).collect();
    lines.sort();
    let mut hasher = Sha256::new();
    for line in lines {
        hasher.update(line.as_bytes());
        hasher.update(b"\n");
    }
    hex::encode(hasher.finalize())
}

/// What the operator asks for: move `graph` from owner `(R, G)` to legacy.
#[derive(Clone, Copy, Debug)]
pub struct ReversePlan<'a> {
    /// The routed named graph.
    pub graph: &'a str,
    /// Catalog revision `R` the graph is routed (and legacy fenced) at.
    pub catalog_revision: u64,
    /// Route generation `G` (catalog `Route.generation`, not the witness's).
    pub generation: u64,
}

impl ReversePlan<'_> {
    fn next(&self) -> Result<(u64, u64)> {
        match (
            self.catalog_revision.checked_add(1),
            self.generation.checked_add(1),
        ) {
            (Some(r), Some(g)) if self.catalog_revision > 0 && self.generation > 0 => Ok((r, g)),
            _ => Err(refused("a reverse needs a nonzero revision and generation")),
        }
    }
}

/// Where the legacy reconciliation journal stands for one graph.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReverseState {
    /// Nothing written yet: legacy is still fenced at `(R, G)`.
    Planned,
    /// Fence at `Reconciling(R+1, G+1)`; no owner fact imported yet.
    Reconciling,
    /// Owner facts imported and the equality proof passed; still fenced.
    Proven,
    /// Fence lifted at `(R+1, G+1)`; legacy serves the graph again.
    Lifted,
}

impl ReverseState {
    /// Stable receipt spelling.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Planned => "planned",
            Self::Reconciling => "reconciling",
            Self::Proven => "proven",
            Self::Lifted => "lifted",
        }
    }

    fn parse(text: &str) -> Result<Self> {
        match text {
            "reconciling" => Ok(Self::Reconciling),
            "proven" => Ok(Self::Proven),
            "lifted" => Ok(Self::Lifted),
            _ => Err(refused(format!("unknown journal state {text:?}"))),
        }
    }
}

/// State-derived receipt: a resumed run reports exactly what a straight run
/// does, because nothing in it depends on which process performed a step.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReverseReceipt {
    /// The reversed graph.
    pub graph: String,
    /// `(R, G)` the owner route was at.
    pub from: (u64, u64),
    /// `(R+1, G+1)` the legacy route is at after the reverse.
    pub to: (u64, u64),
    /// Journaled state.
    pub state: ReverseState,
    /// Number of owner facts reconciled.
    pub owner_facts: usize,
    /// Owner fact counts by source tag (`-` for an untagged write).
    pub sources: BTreeMap<String, usize>,
    /// Whole-graph digest proven equal on both sides.
    pub multiset_sha256: String,
    /// The timestamp recorded at step 2 and used for every imported fact.
    pub at: Option<String>,
    /// Owner witness state; always [`WITNESS_STATE`] until aegis-4qv.4.
    pub witness_state: &'static str,
}

#[path = "routed_reverse_preflight.rs"]
mod preflight;
pub use preflight::{OwnerLock, lock_owner_file, validate_reverse_catalogs};

impl Store {
    /// Every current fact of the named `graph` with its source tag
    /// (step 1). Refuses an unregistered or non-committed graph.
    pub fn export_routed_graph(&self, graph: &str) -> Result<Vec<GraphFact>> {
        let g = self
            .committed_graph_id(graph)?
            .ok_or_else(|| refused("graph is not a committed graph in this file"))?;
        self.graph_facts(g)
    }

    fn graph_facts(&self, g: i64) -> Result<Vec<GraphFact>> {
        let mut stmt = self.conn.prepare(GRAPH_FACTS_SQL)?;
        let rows = stmt
            .query_map([g], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, Vec<u8>>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, Option<String>>(4)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(|(e, a, v, valid_from, source)| {
                Ok(GraphFact {
                    source,
                    fact: SliceFact {
                        subject: self.resolve(e)?,
                        predicate: self.resolve(a)?,
                        object: match Value::from_bytes(&v)? {
                            Value::Ref(id) => Ok(self.resolve(id)?),
                            literal => Err(literal),
                        },
                        valid_from,
                    },
                })
            })
            .collect()
    }

    fn legacy_graph_facts(&self, graph: &str) -> Result<Vec<GraphFact>> {
        match self.committed_graph_id(graph)? {
            Some(g) => self.graph_facts(g),
            None => Ok(Vec::new()),
        }
    }

    fn journal_row(&self, graph: &str) -> Result<Option<JournalRow>> {
        let exists: bool = self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM main.sqlite_master WHERE type = 'table' AND name = ?1)",
            [JOURNAL],
            |r| r.get(0),
        )?;
        if !exists {
            return Ok(None);
        }
        Ok(self
            .conn
            .query_row(
                &format!(
                    "SELECT from_revision, from_generation, to_revision, to_generation, \
                     multiset_sha256, facts, at, state FROM {JOURNAL} WHERE iri = ?1"
                ),
                [graph],
                |r| {
                    Ok(JournalRow {
                        from: (r.get::<_, i64>(0)?, r.get::<_, i64>(1)?),
                        to: (r.get::<_, i64>(2)?, r.get::<_, i64>(3)?),
                        sha: r.get(4)?,
                        facts: r.get(5)?,
                        at: r.get(6)?,
                        state: r.get(7)?,
                    })
                },
            )
            .optional()?)
    }

    /// Read-only: where the reverse of `plan` stands, refusing any state
    /// that does not belong to this plan and these owner facts.
    pub fn reverse_state(
        &self,
        plan: &ReversePlan<'_>,
        owner: &[GraphFact],
    ) -> Result<(ReverseState, Option<String>)> {
        let to = plan.next()?;
        let sha = graph_multiset_sha256(plan.graph, owner);
        let fence = self
            .routed_graph_fence_rows()?
            .into_iter()
            .find(|row| row.0 == plan.graph)
            .map(|row| (row.1, row.2));
        let Some(row) = self.journal_row(plan.graph)? else {
            return match fence {
                Some(at) if at == (plan.catalog_revision, plan.generation) => {
                    if !self.legacy_graph_facts(plan.graph)?.is_empty() {
                        return Err(refused("the legacy graph already holds current facts"));
                    }
                    Ok((ReverseState::Planned, None))
                }
                Some(_) => Err(refused("legacy is fenced at another (R, G); stale reverse")),
                None => Err(refused("legacy is not fenced for this graph")),
            };
        };
        let as_u64 = |(a, b): (i64, i64)| (a.cast_unsigned(), b.cast_unsigned());
        if as_u64(row.from) != (plan.catalog_revision, plan.generation) || as_u64(row.to) != to {
            return Err(refused("a reverse for another (R, G) is journaled here"));
        }
        if row.sha != sha || usize::try_from(row.facts).ok() != Some(owner.len()) {
            return Err(refused("owner facts differ from the journaled reverse"));
        }
        let state = ReverseState::parse(&row.state)?;
        let expected_fence = (state != ReverseState::Lifted).then_some(to);
        if fence != expected_fence {
            return Err(refused("legacy fence does not match the journaled state"));
        }
        Ok((state, Some(row.at)))
    }

    /// Advance the reverse by exactly one step and return the new state.
    /// Each step is one transaction, so a crash leaves the previous state.
    pub fn reverse_step(
        &mut self,
        plan: &ReversePlan<'_>,
        owner: &[GraphFact],
        at: &str,
    ) -> Result<ReverseState> {
        let (state, recorded) = self.reverse_state(plan, owner)?;
        let at = recorded.unwrap_or_else(|| at.to_string());
        let (to_r, to_g) = plan.next()?;
        let (to_r, to_g) = (
            i64::try_from(to_r).map_err(refused)?,
            i64::try_from(to_g).map_err(refused)?,
        );
        if state != ReverseState::Lifted {
            self.ensure_ownership_writable()?;
            self.verify_routed_fence()?;
        }
        match state {
            ReverseState::Planned => {
                let tx = self.conn.unchecked_transaction()?;
                tx.execute_batch(&format!(
                    "CREATE TABLE IF NOT EXISTS {JOURNAL} (
                         iri TEXT PRIMARY KEY,
                         from_revision INTEGER NOT NULL, from_generation INTEGER NOT NULL,
                         to_revision INTEGER NOT NULL, to_generation INTEGER NOT NULL,
                         multiset_sha256 TEXT NOT NULL, facts INTEGER NOT NULL,
                         at TEXT NOT NULL,
                         state TEXT NOT NULL CHECK (state IN ('reconciling','proven','lifted'))
                     );"
                ))?;
                tx.execute(
                    &format!(
                        "INSERT INTO {JOURNAL} VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'reconciling')"
                    ),
                    params![
                        plan.graph,
                        i64::try_from(plan.catalog_revision).map_err(refused)?,
                        i64::try_from(plan.generation).map_err(refused)?,
                        to_r,
                        to_g,
                        graph_multiset_sha256(plan.graph, owner),
                        i64::try_from(owner.len()).map_err(refused)?,
                        at,
                    ],
                )?;
                self.set_fence_epoch(&tx, plan.graph, to_r, to_g)?;
                tx.commit()?;
                Ok(ReverseState::Reconciling)
            }
            ReverseState::Reconciling => {
                self.conn.execute_batch(&format!("SAVEPOINT {SAVEPOINT}"))?;
                match self.import_and_prove(plan.graph, owner, &at, to_r, to_g) {
                    Ok(()) => {
                        self.conn.execute_batch(&format!("RELEASE {SAVEPOINT}"))?;
                        Ok(ReverseState::Proven)
                    }
                    Err(error) => {
                        crate::store::release::rollback_release(&self.conn, SAVEPOINT)?;
                        self.read_model.borrow_mut().clear();
                        Err(error)
                    }
                }
            }
            ReverseState::Proven => {
                // Re-prove before lifting: nothing lifts without equality.
                if graph_multiset_sha256(plan.graph, &self.legacy_graph_facts(plan.graph)?)
                    != graph_multiset_sha256(plan.graph, owner)
                {
                    return Err(refused("legacy no longer equals the owner; staying fenced"));
                }
                let tx = self.conn.unchecked_transaction()?;
                tx.execute(
                    &format!("DELETE FROM {FENCE_TABLE} WHERE iri = ?1"),
                    [plan.graph],
                )?;
                if self.has_source_fence()? {
                    tx.execute(
                        &format!("DELETE FROM {SOURCE_FENCE_TABLE} WHERE graph = ?1"),
                        [plan.graph],
                    )?;
                }
                tx.execute(
                    &format!("UPDATE {JOURNAL} SET state = 'lifted' WHERE iri = ?1"),
                    [plan.graph],
                )?;
                tx.commit()?;
                Ok(ReverseState::Lifted)
            }
            ReverseState::Lifted => Ok(ReverseState::Lifted),
        }
    }

    /// Step 3, inside the caller's savepoint. The fence rows are removed only
    /// for the length of this savepoint; SQLite's single writer means no
    /// other connection can write while they are absent.
    fn import_and_prove(
        &mut self,
        graph: &str,
        owner: &[GraphFact],
        at: &str,
        to_r: i64,
        to_g: i64,
    ) -> Result<()> {
        let held = self.take_fence_rows(graph)?;
        // Recorded creation, so no later open has creator provenance to backfill
        // (a backfill would be a write to a fenced graph).
        self.graph_create_recorded(graph, at, Some("quipu pilot reverse"))?;
        let g = self
            .committed_graph_id(graph)?
            .ok_or_else(|| refused("legacy graph registration failed"))?;
        let mut by_source: BTreeMap<Option<&str>, Vec<super::Datum>> = BTreeMap::new();
        for f in owner {
            let datum = super::Datum {
                entity: self.intern(&f.fact.subject)?,
                attribute: self.intern(&f.fact.predicate)?,
                value: match &f.fact.object {
                    Ok(iri) => Value::Ref(self.intern(iri)?),
                    Err(literal) => literal.clone(),
                },
                valid_from: f.fact.valid_from.clone(),
                valid_to: None,
                op: Op::Assert,
            };
            by_source
                .entry(f.source.as_deref())
                .or_default()
                .push(datum);
        }
        for (source, datums) in by_source {
            self.transact_to_graph_scoped(&datums, at, None, source, g, source)?;
        }
        if graph_multiset_sha256(graph, &self.graph_facts(g)?)
            != graph_multiset_sha256(graph, owner)
        {
            return Err(refused(
                "imported legacy graph differs from the owner; staying Reconciling",
            ));
        }
        self.restore_fence_rows(&held, to_r, to_g)?;
        self.conn.execute(
            &format!("UPDATE {JOURNAL} SET state = 'proven' WHERE iri = ?1"),
            [graph],
        )?;
        Ok(())
    }

    /// Run the reverse to completion (resuming any recorded state), or with
    /// `execute == false` report the state without writing anything.
    pub fn reverse_routed_graph(
        &mut self,
        plan: &ReversePlan<'_>,
        owner: &[GraphFact],
        at: &str,
        execute: bool,
    ) -> Result<ReverseReceipt> {
        if execute {
            while self.reverse_step(plan, owner, at)? != ReverseState::Lifted {}
        }
        self.reverse_receipt(plan, owner)
    }

    /// The state-derived receipt for `plan` (read-only).
    pub fn reverse_receipt(
        &self,
        plan: &ReversePlan<'_>,
        owner: &[GraphFact],
    ) -> Result<ReverseReceipt> {
        let (state, at) = self.reverse_state(plan, owner)?;
        let mut sources = BTreeMap::new();
        for f in owner {
            *sources
                .entry(f.source.clone().unwrap_or_else(|| "-".into()))
                .or_insert(0) += 1;
        }
        Ok(ReverseReceipt {
            graph: plan.graph.to_string(),
            from: (plan.catalog_revision, plan.generation),
            to: plan.next()?,
            state,
            owner_facts: owner.len(),
            sources,
            multiset_sha256: graph_multiset_sha256(plan.graph, owner),
            at,
            witness_state: WITNESS_STATE,
        })
    }
}

/// Fence rows lifted for the length of the step-3 savepoint.
struct HeldFence {
    graph: String,
    sources: Vec<String>,
}

impl Store {
    fn has_source_fence(&self) -> Result<bool> {
        Ok(self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM main.sqlite_master WHERE type = 'table' AND name = ?1)",
            [SOURCE_FENCE_TABLE],
            |r| r.get(0),
        )?)
    }

    /// Step 2: move the graph's fence row and its producer source rows to
    /// the new epoch. The triggers keep refusing every other writer.
    fn set_fence_epoch(
        &self,
        tx: &rusqlite::Transaction<'_>,
        graph: &str,
        revision: i64,
        generation: i64,
    ) -> Result<()> {
        tx.execute(
            &format!(
                "UPDATE {FENCE_TABLE} SET catalog_revision = ?2, generation = ?3 WHERE iri = ?1"
            ),
            params![graph, revision, generation],
        )?;
        if self.has_source_fence()? {
            tx.execute(
                &format!(
                    "UPDATE {SOURCE_FENCE_TABLE} SET catalog_revision = ?2, generation = ?3 \
                     WHERE graph = ?1"
                ),
                params![graph, revision, generation],
            )?;
        }
        Ok(())
    }

    fn take_fence_rows(&self, graph: &str) -> Result<HeldFence> {
        let sources = if self.has_source_fence()? {
            let mut stmt = self.conn.prepare(&format!(
                "SELECT source FROM {SOURCE_FENCE_TABLE} WHERE graph = ?1 ORDER BY source"
            ))?;
            let rows = stmt
                .query_map([graph], |r| r.get::<_, String>(0))?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            self.conn.execute(
                &format!("DELETE FROM {SOURCE_FENCE_TABLE} WHERE graph = ?1"),
                [graph],
            )?;
            rows
        } else {
            Vec::new()
        };
        self.conn.execute(
            &format!("DELETE FROM {FENCE_TABLE} WHERE iri = ?1"),
            [graph],
        )?;
        Ok(HeldFence {
            graph: graph.to_string(),
            sources,
        })
    }

    fn restore_fence_rows(&self, held: &HeldFence, revision: i64, generation: i64) -> Result<()> {
        let now = "strftime('%Y-%m-%dT%H:%M:%SZ','now')";
        self.conn.execute(
            &format!(
                "INSERT INTO {FENCE_TABLE}(iri, catalog_revision, generation, fenced_at) \
                 VALUES (?1, ?2, ?3, {now})"
            ),
            params![held.graph, revision, generation],
        )?;
        for source in &held.sources {
            self.conn.execute(
                &format!(
                    "INSERT INTO {SOURCE_FENCE_TABLE}(source, graph, catalog_revision, generation, fenced_at) \
                     VALUES (?1, ?2, ?3, ?4, {now})"
                ),
                params![source, held.graph, revision, generation],
            )?;
        }
        Ok(())
    }
}

struct JournalRow {
    from: (i64, i64),
    to: (i64, i64),
    sha: String,
    facts: i64,
    at: String,
    state: String,
}

#[cfg(test)]
#[path = "routed_reverse_tests.rs"]
mod tests;
