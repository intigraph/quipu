//! One read transaction for all local SQL reads of a query.
//!
//! This pins each SQLite file when it is first read; it does not claim a
//! cross-file commit order for attached databases. Existing caller-owned
//! transactions retain their own lifetime and are never committed here.

use super::*;

/// Execute a SPARQL query with temporal context from one SQLite read snapshot
/// per participating file. Existing attached-file restrictions still apply.
///
/// Enforces the store's query budget (`[quipu.search] query_timeout_ms`): unless `ctx.deadline` is already set by the caller, a deadline
/// is derived from the store config, a `SQLite` progress handler interrupts
/// statement execution past it, and the evaluator checks it between operators.
/// Past-deadline failures surface as [`Error::QueryTimeout`], not whatever
/// error the interrupt happened to produce.
///
/// The progress handler is installed per top-level query on the store's one
/// connection; a nested `query_temporal` without its own deadline runs under
/// the outer query's handler and budget, which is the intended accounting.
pub fn query_temporal(store: &Store, sparql: &str, ctx: &TemporalContext) -> Result<QueryResult> {
    if !store.conn.is_autocommit() {
        return query_temporal_inner(store, sparql, ctx);
    }
    let transaction = store.conn.unchecked_transaction()?;
    let result = query_temporal_inner(store, sparql, ctx)?;
    transaction.commit()?;
    Ok(result)
}

#[cfg(test)]
thread_local! {
    static AFTER_UNION_LEFT: std::cell::RefCell<Option<Box<dyn FnOnce()>>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
pub(super) fn set_after_union_left(hook: Option<Box<dyn FnOnce()>>) {
    AFTER_UNION_LEFT.with(|slot| *slot.borrow_mut() = hook);
}

#[cfg(test)]
pub(super) fn after_union_left() {
    let hook = AFTER_UNION_LEFT.with(|slot| slot.borrow_mut().take());
    if let Some(hook) = hook {
        hook();
    }
}

fn query_temporal_inner(store: &Store, sparql: &str, ctx: &TemporalContext) -> Result<QueryResult> {
    if ctx.as_of_tx.is_some() && !store.attachments().is_empty() {
        return Err(Error::Store(
            "as_of_tx is refused for a store with attached databases: transaction IDs are \
             file-local and have no cross-file ordering; see \
             docs/design/multi-db-composition.md §6"
                .to_string(),
        ));
    }

    let parsed = crate::sparql_structure::parse_query(sparql_parser(), sparql)?
        .map_err(|e| Error::InvalidValue(format!("SPARQL parse error: {e}")))?;

    let started = crate::time::Stopwatch::start();
    root_scope::check(&parsed)?;
    // Precedence, widest-scope last: an explicit caller deadline wins; then the
    // request deadline stamped at ADMISSION, which already includes whatever
    // this request spent queueing (aegis-raq1ok); then the per-query default,
    // whose clock starts here and so cannot see queue time at all.
    let deadline = ctx
        .deadline
        .or_else(crate::time::request_deadline)
        .or_else(|| {
            let limit_ms = store.search_config().query_timeout_ms;
            (limit_ms > 0).then(|| crate::time::Deadline::after_millis(limit_ms))
        });
    let row_cap = ctx.row_cap.or_else(|| {
        let cap = store.search_config().max_join_rows;
        (cap > 0).then_some(cap)
    });
    let ctx = TemporalContext {
        deadline,
        row_cap,
        ..ctx.clone()
    };
    let _guard = deadline
        .map(|dl| ProgressGuard::install(&store.conn, dl))
        .transpose()?;

    let base_iri = parsed.base_iri().map(ToString::to_string);
    let result = filter::with_base_iri(base_iri.as_deref(), || {
        // A query that can read no attached graph answers as the unattached
        // store does, so it keeps that store's fast paths (aegis-xacr). The
        // decision reads the store, so it runs under the query's own budget
        // and its errors are normalised like the evaluator's.
        let dataset = match &parsed {
            Query::Select { dataset, .. }
            | Query::Construct { dataset, .. }
            | Query::Describe { dataset, .. }
            | Query::Ask { dataset, .. } => dataset.as_ref(),
        };
        let scoped = apply_dataset(store, dataset, &ctx)?;
        let composes = attach_scope::composes(store, &parsed, &scoped.graph, &ctx)?;
        let _composition = store.scope_composition(composes);
        eval_parsed(store, parsed, &ctx)
    });
    match result {
        // Any failure past the deadline is reported as the timeout it is: the
        // proximate error (a SQLITE_INTERRUPT, or the evaluator's own check)
        // is just the mechanism that stopped the query.
        Err(_) if deadline.is_some_and(|dl| dl.passed()) => Err(Error::QueryTimeout {
            elapsed_ms: started.elapsed_ms(),
            limit_ms: deadline
                .map(|dl| dl.millis_from(&started))
                .unwrap_or_default(),
        }),
        other => other,
    }
}
