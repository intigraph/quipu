//! Private scratch work never owns the serving mutex.
use super::SharedStore;
use crate::base::{AppError, blocking};
use quipu::owl::snapshot::Snapshot;
use quipu::write_observation::{Observation, Path, Stage};

pub(super) struct Published {
    pub snapshot: Snapshot,
    pub permit: tokio::sync::SemaphorePermit<'static>,
    pub applied: usize,
    pub max_writer_batch_micros: u128,
}

pub(super) async fn publish(
    store: SharedStore,
    mut snapshot: Snapshot,
    mut permit: tokio::sync::SemaphorePermit<'static>,
) -> Result<Published, AppError> {
    let mut applied = 0;
    let mut max_writer_batch_micros = 0;
    let test_key = std::sync::Arc::as_ptr(&store) as usize;
    loop {
        let observation = Observation::new(Path::OwlPage, crate::request_middleware::request_id());
        let completion = observation.completion();
        let preparing = observation.clone();
        let result = blocking(move || {
            preparing.scope(|| {
                match private_phase(test_key, "prepare", || snapshot.prepare_page()) {
                    Ok(page) => Ok((snapshot, permit, page, completion)),
                    Err(error) => {
                        completion.finish(false);
                        Err(error.into())
                    }
                }
            })
        })
        .await?;
        let (mut current, held_permit, prepared, completion) = result;
        let Some(prepared) = prepared else {
            completion.finish(true);
            snapshot = current;
            permit = held_permit;
            break;
        };
        let queued = std::time::Instant::now();
        let dispatch = observation.stage(Stage::QueuedDispatch);
        let granted = std::sync::Arc::new(std::sync::OnceLock::new());
        let stamp = granted.clone();
        let writer = store.clone();
        let result = crate::admission::write_blocking_observed(stamp, move || {
            dispatch.finish(true);
            observation.admission_granted(queued, granted.get().copied());
            let result = observation.scope(|| {
                let waiting = quipu::write_observation::stage(Stage::LockWait);
                let mut live = writer.lock();
                waiting.finish(true);
                let held = quipu::write_observation::stage(Stage::LockHeld);
                let started = std::time::Instant::now();
                let committed = prepared.apply(&mut live);
                let work = if committed.is_ok() {
                    live.take_deferred_embed()
                } else {
                    None
                };
                drop(live);
                let micros = started.elapsed().as_micros();
                held.finish(committed.is_ok());
                // The live guard is gone before private SQLite reads or saves.
                // Preserve write admission across acknowledgement/deferred work,
                // exactly as before; other mutex users can now make progress.
                let n = private_phase(test_key, "acknowledge", || {
                    current.reconcile_page(committed)
                })?;
                if let Some(work) = work {
                    quipu::write_observation::observe(Stage::Deferred, || {
                        crate::tools::finish_deferred_embed(&writer, &work)
                    })?;
                }
                Ok((current, held_permit, n, micros))
            });
            completion.finish(result.is_ok());
            result
        })
        .await?;
        (snapshot, permit) = (result.0, result.1);
        applied += result.2;
        max_writer_batch_micros = max_writer_batch_micros.max(result.3);
    }
    let observation = Observation::new(Path::OwlFinish, crate::request_middleware::request_id());
    let completion = observation.completion();
    let preparing = observation.clone();
    let (snapshot, permit, prepared, completion) = blocking(move || {
        preparing.scope(|| match snapshot.prepare_finish() {
            Ok(prepared) => Ok((snapshot, permit, prepared, completion)),
            Err(error) => {
                completion.finish(false);
                Err(error.into())
            }
        })
    })
    .await?;
    let queued = std::time::Instant::now();
    let dispatch = observation.stage(Stage::QueuedDispatch);
    let granted = std::sync::Arc::new(std::sync::OnceLock::new());
    let stamp = granted.clone();
    crate::admission::write_blocking_observed(stamp, move || {
        dispatch.finish(true);
        observation.admission_granted(queued, granted.get().copied());
        let result = observation.scope(|| {
            let waiting = quipu::write_observation::stage(Stage::LockWait);
            let mut live = store.lock();
            waiting.finish(true);
            let held = quipu::write_observation::stage(Stage::LockHeld);
            let started = std::time::Instant::now();
            let committed = prepared.apply(&mut live);
            let work = if committed.is_ok() {
                live.take_deferred_embed()
            } else {
                None
            };
            drop(live);
            max_writer_batch_micros = max_writer_batch_micros.max(started.elapsed().as_micros());
            held.finish(committed.is_ok());
            snapshot.reconcile_finish(committed)?;
            if let Some(work) = work {
                quipu::write_observation::observe(Stage::Deferred, || {
                    crate::tools::finish_deferred_embed(&store, &work)
                })?;
            }
            Ok(Published {
                snapshot,
                permit,
                applied,
                max_writer_batch_micros,
            })
        });
        completion.finish(result.is_ok());
        result
    })
    .await
}

// The test barrier travels with the private operation, rather than sampling
// process counts or adding real multi-second sleeps to a performance test.
fn private_phase<T>(key: usize, phase: &str, f: impl FnOnce() -> T) -> T {
    #[cfg(test)]
    test_gate::wait(key, phase);
    #[cfg(not(test))]
    let _ = (key, phase);
    f()
}

#[cfg(test)]
#[path = "owl_publish_tests.rs"]
mod test_gate;
