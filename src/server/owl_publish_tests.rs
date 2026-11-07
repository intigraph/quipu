use super::*;
use std::sync::{Mutex, mpsc};
use std::time::Duration;

struct Gate {
    key: usize,
    phase: &'static str,
    reached: mpsc::SyncSender<()>,
    release: mpsc::Receiver<()>,
}
static GATE: Mutex<Option<Gate>> = Mutex::new(None);

pub(super) fn wait(key: usize, phase: &str) {
    let gate = {
        let mut gate = GATE.lock().unwrap();
        if gate
            .as_ref()
            .is_some_and(|g| g.key == key && g.phase == phase)
        {
            gate.take()
        } else {
            None
        }
    };
    if let Some(gate) = gate {
        gate.reached.send(()).unwrap();
        gate.release.recv_timeout(Duration::from_secs(30)).unwrap();
    }
}

fn probe(store: SharedStore) -> (std::thread::JoinHandle<()>, mpsc::Receiver<bool>) {
    let (sender, receiver) = mpsc::sync_channel(1);
    let thread = std::thread::spawn(move || {
        let mut live = store.lock();
        let result = quipu::tool_knot(
            &mut live,
            &serde_json::json!({
                "turtle": "<urn:stage-control> <urn:stage-predicate> \"ok\" ."
            }),
        );
        sender.send(result.is_ok()).unwrap();
    });
    (thread, receiver)
}

#[tokio::test]
async fn slow_private_preparation_and_acknowledgement_leave_real_writes_free() {
    let _serial = super::super::tests::SERIAL.lock().await;
    for phase in ["prepare", "acknowledge"] {
        let (_dir, store) = super::super::tests::seeded();
        let (reached_tx, reached_rx) = mpsc::sync_channel(1);
        let (release_tx, release_rx) = mpsc::sync_channel(1);
        *GATE.lock().unwrap() = Some(Gate {
            key: std::sync::Arc::as_ptr(&store) as usize,
            phase,
            reached: reached_tx,
            release: release_rx,
        });
        let materializer = store.clone();
        let job = tokio::spawn(async move {
            super::super::materialize(materializer, serde_json::json!({"action":"materialize"}))
                .await
        });
        let reached =
            tokio::task::spawn_blocking(move || reached_rx.recv_timeout(Duration::from_secs(30)))
                .await
                .unwrap();
        if reached.is_err() {
            let _ = release_tx.send(());
            *GATE.lock().unwrap() = None;
            let _ = job.await.unwrap().unwrap();
            panic!("private operation barrier was not reached");
        }
        let (thread, receiver) = probe(store);
        let wrote_while_private_work_paused = receiver.recv_timeout(Duration::from_secs(5));
        // Release first even if the assertion will fail, so a bad lock scope
        // cannot strand an uncancellable child behind the barrier.
        release_tx.send(()).unwrap();
        thread.join().unwrap();
        let _ = job.await.unwrap().unwrap();
        assert!(wrote_while_private_work_paused.unwrap(), "{phase}");
    }
}

#[test]
fn write_probe_negative_control_really_detects_a_held_live_guard() {
    let (_dir, store) = super::super::tests::seeded();
    let guard = store.lock();
    let (thread, receiver) = probe(store.clone());
    assert!(receiver.recv_timeout(Duration::from_millis(50)).is_err());
    drop(guard);
    assert!(receiver.recv_timeout(Duration::from_secs(5)).unwrap());
    thread.join().unwrap();
}
