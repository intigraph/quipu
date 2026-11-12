//! One document per paced turn; canonicalization never holds the writer mutex.
use super::SharedStore;

pub(super) fn spawn(store: SharedStore) {
    tokio::spawn(async move {
        let mut attempted = 0usize;
        loop {
            let state = store.clone();
            let result = tokio::task::spawn_blocking(move || {
                one_turn(&state, quipu::shape_mapping::RegistryDigestWork::compute)
            })
            .await;
            match result {
                Ok(Ok(false)) => return,
                Ok(Ok(true)) => {
                    attempted += 1;
                    if attempted.is_multiple_of(32) {
                        eprintln!(
                            "registry digest backfill: {attempted} version attempts this run"
                        );
                    }
                }
                Ok(Err(error)) => {
                    eprintln!("registry digest backfill paused; progress retained: {error}");
                    return;
                }
                Err(error) => {
                    eprintln!("registry digest backfill task failed; progress retained: {error}");
                    return;
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    });
}

fn one_turn(
    state: &SharedStore,
    compute: impl FnOnce(
        quipu::shape_mapping::RegistryDigestWork,
    ) -> quipu::shape_mapping::RegistryDigestResult,
) -> quipu::Result<bool> {
    let work = { state.lock().next_registry_digest_work()? };
    let Some(work) = work else {
        eprintln!(
            "registry digest backfill: {:?}",
            state.lock().registry_digest_backfill_status()?
        );
        return Ok(false);
    };
    // The snapshot guard has dropped before RDF parsing/canonicalization.
    let computed = compute(work);
    state.lock().apply_registry_digest_result(computed)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonicalization_releases_the_real_writer_mutex() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("store.db");
        let native = quipu::Store::open(path.to_str().unwrap()).unwrap();
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute("INSERT INTO shapes(name,turtle,loaded_at,valid_from,tx) VALUES('legacy','<urn:s> <urn:p> <urn:o> .','before','before',7)",[]).unwrap();
        drop(conn);
        let state = std::sync::Arc::new(crate::StoreHandle::writer_only(native));
        let worker = state.clone();
        let (started, seen) = std::sync::mpsc::channel();
        let (release, wait) = std::sync::mpsc::channel();
        let thread = std::thread::spawn(move || {
            one_turn(&worker, move |work| {
                started.send(()).unwrap();
                wait.recv_timeout(std::time::Duration::from_secs(5))
                    .unwrap();
                work.compute()
            })
        });
        seen.recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        let guard = state.writer.try_lock();
        let available = guard.is_some();
        drop(guard);
        release.send(()).unwrap();
        assert!(thread.join().unwrap().unwrap());
        assert!(available, "backfill calculation held the real writer mutex");
        assert!(
            state
                .lock()
                .current_shape_pins(quipu::shape_mapping::RegistryKind::Shapes, "legacy")
                .is_ok()
        );
    }
}
