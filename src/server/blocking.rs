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

/// Detached HTTP jobs retain their token after their handler has returned.
pub(crate) fn spawn_tracked<T: Send + 'static>(
    f: impl FnOnce() -> T + Send + 'static,
) -> tokio::task::JoinHandle<T> {
    let work = request_middleware::request_store_work().map(|context| {
        context
            .registry
            .start(context.lane, quipu::metrics::inflight::Phase::Waiting)
    });
    tokio::task::spawn_blocking(move || {
        if let Some(work) = &work {
            work.activate();
        }
        let _work = work;
        f()
    })
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
    let search = crate::search_observation::current();
    // On a deep stack: request work parses and walks caller-supplied SPARQL,
    // whose recursion follows its structure, and the pool's default stack
    // aborted the process on a few kilobytes of nesting (aegis-xcvb5z). The
    // structural bound in `sparql_structure` keeps requests inside this stack;
    // the stack keeps the bound's margin. Identity, write kind and write
    // provenance are thread-scoped, so they are re-established on the new thread.
    match tokio::task::spawn_blocking(move || {
        quipu::sparql_structure::on_deep_stack(move || {
            quipu::transaction_auth::with_identity(identity, || {
                quipu::write_provenance::scoped(provenance, || {
                    quipu::write_kind::scoped(kind, || match search {
                        Some(call) => call.scope(f),
                        None => f(),
                    })
                })
            })
        })
    })
    .await
    {
        Ok(Ok(result)) => result,
        Ok(Err(e)) => Err(AppError(e, None)),
        // Only reachable if the handler panicked; the mutex is then poisoned and
        // the process is not going to recover on its own either way.
        Err(e) => Err(AppError(
            quipu::Error::InvalidValue(format!("request handler failed: {e}")),
            None,
        )),
    }
}
