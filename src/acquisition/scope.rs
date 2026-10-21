//! Bounded physical scope measurement for a direct-main acquisition
//! (aegis-ehf G1).
//!
//! For each named graph at ONE applied pin, counts the physical
//! `DISTINCT(e,a,v)` rows a direct-main acquisition would read (the same
//! predicate, [`main_graph::PHYSICAL_ROWS`]) and the bytes of their values,
//! stopping at [`ROW_CAP`] + 1. A graph over the cap is reported as such, never
//! as a count. It writes nothing and returns no fact content.
//!
//! Every statement runs under an explicit SQLite VM-step budget and wall
//! deadline. Exhausting either aborts the whole measurement with the phase that
//! was running; there is no partial result.

use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use oxrdf::NamedNode;
use serde::{Deserialize, Serialize};

use super::{invalid, main_graph};
use crate::{Error, Result, Store};

/// Distinct from every acquisition contract.
pub const CONTRACT: &str = "quipu-acquisition-scope/v1";

/// The unchanged acquisition row cap per graph.
pub const ROW_CAP: u64 = 100_000;

/// SQLite VM steps between progress checks.
const PROGRESS_STRIDE: i32 = 1000;

/// Exact graph selection, pin and the run's explicit resource budget.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    /// Expected main lineage; a mismatch refuses.
    pub source_store_id: String,
    /// Exact registered named graph IRIs.
    pub graphs: Vec<String>,
    /// Applied pin; absent means the observed head.
    pub tx: Option<i64>,
    /// Total SQLite VM steps for the whole run, counted in strides of 1000
    /// steps per statement (a statement shorter than one stride is not counted).
    pub max_vm_steps: u64,
    /// Wall-clock budget for the whole run.
    pub deadline_ms: u64,
}

/// One graph's physical scope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum GraphScope {
    /// Within the cap: exact counts.
    Measured {
        /// Physical `DISTINCT(e,a,v)` rows at the applied pin.
        rows: u64,
        /// Sum of the encoded value bytes of those rows.
        value_bytes: u64,
        /// Largest single encoded value.
        max_value_bytes: u64,
    },
    /// More than [`ROW_CAP`] rows; the true count is unknown and not reported.
    ExceedsCap,
    /// Refused by direct-main admission before any fact was read.
    Refused {
        /// Admission reason.
        reason: String,
    },
}

/// The measurement. It exists only if every graph was answered within budget.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Measurement {
    /// [`CONTRACT`].
    pub schema: String,
    /// Main lineage observed inside the read transaction.
    pub source_store_id: String,
    /// Committed head observed inside the read transaction.
    pub source_head_tx: i64,
    /// The single pin every graph was measured at.
    pub applied_tx: i64,
    /// Row cap per graph.
    pub row_cap: u64,
    /// Budget the run was given.
    pub max_vm_steps: u64,
    /// Budget the run was given.
    pub deadline_ms: u64,
    /// SQLite VM steps counted against the budget: each statement's steps in
    /// whole strides of 1000, so statements shorter than one stride count 0.
    pub vm_steps_used: u64,
    /// Compiled producer version.
    pub producer_version: String,
    /// Compiled source commit.
    pub producer_git_sha: String,
    /// Whether the producer was built from modified source.
    pub producer_git_dirty: bool,
    /// Every requested graph.
    pub graphs: BTreeMap<String, GraphScope>,
}

/// Which statement the budget interrupted.
#[derive(Clone, Copy)]
enum Phase {
    Identity,
    Metadata,
    PhysicalRows,
}

impl Phase {
    fn name(self) -> &'static str {
        match self {
            Self::Identity => "identity",
            Self::Metadata => "graph_metadata",
            Self::PhysicalRows => "physical_distinct",
        }
    }
}

/// The installed progress handler. Dropping it removes the handler, so no exit
/// path (error, early return, unwind) leaves the budget on the connection.
struct Budget<'s> {
    store: &'s Store,
    steps: Arc<AtomicU64>,
    exhausted: Arc<AtomicBool>,
    deadline: crate::time::Deadline,
}

impl<'s> Budget<'s> {
    fn install(store: &'s Store, request: &Request) -> Result<Self> {
        let steps = Arc::new(AtomicU64::new(0));
        let exhausted = Arc::new(AtomicBool::new(false));
        let deadline = crate::time::Deadline::after_millis(request.deadline_ms);
        let max = request.max_vm_steps;
        let (counter, flag) = (Arc::clone(&steps), Arc::clone(&exhausted));
        // A budget that cannot be installed refuses the run; it never runs unbudgeted.
        store.conn.progress_handler(
            PROGRESS_STRIDE,
            Some(move || {
                let used = counter
                    .fetch_add(u64::from(PROGRESS_STRIDE.unsigned_abs()), Ordering::Relaxed)
                    + u64::from(PROGRESS_STRIDE.unsigned_abs());
                let stop = used > max || deadline.passed();
                if stop {
                    flag.store(true, Ordering::Relaxed);
                }
                stop
            }),
        )?;
        Ok(Self {
            store,
            steps,
            exhausted,
            deadline,
        })
    }

    /// The wall deadline at a phase boundary. The progress handler sees it only
    /// once per 1000 VM steps of one statement, so a run of short but slow
    /// statements could otherwise finish past it and still report a result
    /// (ian, #705 review).
    fn check_deadline(&self, phase: Phase) -> Result<()> {
        if self.deadline.passed() {
            return Err(invalid(format!(
                "scope wall deadline exceeded during {}; completeness UNKNOWN",
                phase.name()
            )));
        }
        Ok(())
    }
}

impl Drop for Budget<'_> {
    fn drop(&mut self) {
        let _ = self.store.conn.progress_handler(0, None::<fn() -> bool>);
    }
}

/// Measure the physical scope of `request.graphs` at one pin.
///
/// # Errors
/// Refuses a malformed or mismatched request or an unknown pin. Exhausting
/// the VM-step or wall budget refuses the whole run, naming the phase;
/// completeness is then UNKNOWN and nothing is returned.
pub fn measure(store: &Store, request: &Request) -> Result<Measurement> {
    if request.graphs.is_empty() || request.graphs.len() > 16 {
        return Err(invalid("scope requires 1..=16 explicit named graphs"));
    }
    if request.max_vm_steps == 0 || request.deadline_ms == 0 {
        return Err(invalid(
            "scope requires an explicit nonzero VM-step and wall budget",
        ));
    }
    let mut seen = BTreeSet::new();
    for iri in &request.graphs {
        NamedNode::new(iri.clone()).map_err(|e| invalid(format!("graph IRI: {e}")))?;
        if !seen.insert(iri) {
            return Err(invalid("duplicate scope graph"));
        }
    }
    let budget = Budget::install(store, request)?;
    measure_within(store, request, &budget)
}

fn measure_within(store: &Store, request: &Request, budget: &Budget<'_>) -> Result<Measurement> {
    let phase = Cell::new(Phase::Identity);
    let interrupted = |error: Error| -> Error {
        if budget.exhausted.load(Ordering::Relaxed) {
            invalid(format!(
                "scope budget exhausted during {} after {} VM steps; completeness UNKNOWN",
                phase.get().name(),
                budget.steps.load(Ordering::Relaxed)
            ))
        } else {
            error
        }
    };
    let read = store
        .conn
        .unchecked_transaction()
        .map_err(|e| interrupted(e.into()))?;
    let source_store_id = store.store_id().map_err(interrupted)?;
    if source_store_id != request.source_store_id {
        return Err(invalid("scope source store identity mismatch"));
    }
    let head = store.latest_tx_id().map_err(interrupted)?;
    let mut pin = serde_json::json!({});
    if let Some(tx) = request.tx {
        pin["tx"] = serde_json::json!(tx);
    }
    let applied_tx = crate::read_pin::requested_tx(store, &pin)
        .map_err(interrupted)?
        .unwrap_or(head);
    if applied_tx == 0 {
        return Err(invalid("scope source has no committed transaction"));
    }
    budget.check_deadline(Phase::Identity)?;
    let mut graphs = BTreeMap::new();
    for iri in &request.graphs {
        phase.set(Phase::Metadata);
        let admitted = main_graph::metadata(store, iri);
        budget.check_deadline(Phase::Metadata)?;
        let graph = match admitted {
            Ok((graph, _)) => graph,
            Err(Error::InvalidValue(reason) | Error::PolicyDenied(reason))
                if !budget.exhausted.load(Ordering::Relaxed) =>
            {
                graphs.insert(iri.clone(), GraphScope::Refused { reason });
                continue;
            }
            Err(error) => return Err(interrupted(error)),
        };
        phase.set(Phase::PhysicalRows);
        let (rows, value_bytes, max_value_bytes): (i64, i64, i64) = store
            .conn
            .query_row(
                &format!(
                    "SELECT count(*), COALESCE(sum(length(v)),0), COALESCE(max(length(v)),0) \
                     FROM (SELECT DISTINCT e,a,v {} LIMIT ?3)",
                    main_graph::PHYSICAL_ROWS
                ),
                rusqlite::params![graph, applied_tx, ROW_CAP.cast_signed() + 1],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .map_err(|e| interrupted(e.into()))?;
        budget.check_deadline(Phase::PhysicalRows)?;
        let scope = if rows.unsigned_abs() > ROW_CAP {
            GraphScope::ExceedsCap
        } else {
            GraphScope::Measured {
                rows: rows.unsigned_abs(),
                value_bytes: value_bytes.unsigned_abs(),
                max_value_bytes: max_value_bytes.unsigned_abs(),
            }
        };
        graphs.insert(iri.clone(), scope);
    }
    read.commit().map_err(|e| interrupted(e.into()))?;
    // Final boundary: nothing is reported if the run ended past its deadline.
    budget.check_deadline(phase.get())?;
    Ok(Measurement {
        schema: CONTRACT.into(),
        source_store_id,
        source_head_tx: head,
        applied_tx,
        row_cap: ROW_CAP,
        max_vm_steps: request.max_vm_steps,
        deadline_ms: request.deadline_ms,
        vm_steps_used: budget.steps.load(Ordering::Relaxed),
        producer_version: env!("CARGO_PKG_VERSION").into(),
        producer_git_sha: env!("QUIPU_GIT_SHA").into(),
        producer_git_dirty: env!("QUIPU_GIT_DIRTY") == "true",
        graphs,
    })
}

#[cfg(test)]
#[path = "scope_tests.rs"]
mod tests;
