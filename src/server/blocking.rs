//! Blocking work ownership, including generic HTTP handlers outside admission.
use super::AppError;
use crate::{auth, request_middleware};

/// Track a generic handler before it enters the uncancellable blocking pool.
pub(crate) async fn blocking<T, F>(f: F) -> Result<T, AppError>
where
    F: FnOnce() -> Result<T, AppError> + Send + 'static,
    T: Send + 'static,
{
    let work = request_middleware::request_store_work().map(|context| {
        context
            .registry
            .start(context.lane, quipu::metrics::inflight::Phase::Waiting)
    });
    blocking_pretracked(move || {
        if let Some(work) = &work {
            work.activate();
        }
        let _work = work;
        f()
    })
    .await
}

/// Admission already owns a token spanning its queue; never count it twice.
pub(crate) async fn blocking_pretracked<T, F>(f: F) -> Result<T, AppError>
where
    F: FnOnce() -> Result<T, AppError> + Send + 'static,
    T: Send + 'static,
{
    let identity = auth::request_identity();
    let kind = request_middleware::request_write_kind();
    let provenance = request_middleware::request_write_provenance();
    // On a deep stack: request work parses and walks caller-supplied SPARQL,
    // whose recursion follows its structure, and the pool's default stack
    // aborted the process on a few kilobytes of nesting (aegis-xcvb5z). The
    // structural bound in `sparql_structure` keeps requests inside this stack;
    // the stack keeps the bound's margin. Identity, write kind and write
    // provenance are thread-scoped, so they are re-established on the new thread.
    match tokio::task::spawn_blocking(move || {
        quipu::sparql_structure::on_deep_stack(move || {
            quipu::transaction_auth::with_identity(identity, || {
                quipu::write_provenance::scoped(provenance, || quipu::write_kind::scoped(kind, f))
            })
        })
    })
    .await
    {
        Ok(Ok(result)) => result,
        Ok(Err(e)) => Err(AppError(e)),
        // Only reachable if the handler panicked; the mutex is then poisoned and
        // the process is not going to recover on its own either way.
        Err(e) => Err(AppError(quipu::Error::InvalidValue(format!(
            "request handler failed: {e}"
        )))),
    }
}
