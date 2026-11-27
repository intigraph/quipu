//! Phase 3 S2a (aegis-n0wc.5): a REAL process death at each durable edge.
//! The parent re-runs this test binary on the ignored child below. The child
//! opens the fixture owners and runs one batch with a kill point armed; at
//! that point it reports, flushes and blocks while holding every writer and
//! lease. The parent SIGKILLs it, then reopens the files and recovers.
//! LIMIT: the kill lands between durable statements, not inside a SQLite
//! commit (SQLite's own atomic commit covers that); no power loss.
use super::tests::*;
use super::*;
use std::{
    io::{BufRead, BufReader},
    process::{Command, Stdio},
    sync::mpsc,
    time::Duration,
};

const DIR_ENV: &str = "QUIPU_PARTICIPANT_KILL_DIR";
pub(super) const KILL_ENV: &str = "QUIPU_PARTICIPANT_KILL_AT";
pub(super) const MARKER: &str = "PARTICIPANT-KILL-POINT-REACHED";

fn batch_for<'a>(
    files: &Files,
    alpha: &'a GraphSetOwner,
    beta: &'a GraphSetOwner,
) -> ParticipantBatch<'a> {
    ParticipantBatch {
        batch_id: "batch-kill".into(),
        members: members(files, &[alpha, beta], &[0, 1], "v1"),
    }
}

/// Child side: run the batch until the armed kill point blocks it.
#[test]
#[ignore = "run only as the child process of the participant SIGKILL rows"]
fn participant_kill_child() {
    let Ok(dir) = std::env::var(DIR_ENV) else {
        return;
    };
    let files = describe(Path::new(&dir));
    let (alpha, beta) = (open(&files, 0), open(&files, 1));
    let result = as_writer(|| commit(&batch_for(&files, &alpha, &beta)));
    // Reaching here means the kill point never fired: report it loudly.
    println!("PARTICIPANT-KILL-POINT-MISSED {result:?}");
}

/// Spawn the child armed at `point`, wait for its marker, then SIGKILL it.
fn kill_child_at(root: &Path, point: CrashPoint) {
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "store::graph_owner::graph_set::participant::kill_tests::participant_kill_child",
            "--exact",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(DIR_ENV, root)
        .env(KILL_ENV, format!("{point:?}"))
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let stdout = child.stdout.take().unwrap();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout)
            .lines()
            .map_while(std::result::Result::ok)
        {
            if line.contains(MARKER) || line.contains("KILL-POINT-MISSED") {
                let _ = tx.send(line);
                return;
            }
        }
    });
    let line = rx.recv_timeout(Duration::from_secs(120));
    // Kill unconditionally, also on timeout, so no child outlives the row.
    child.kill().unwrap();
    let status = child.wait().unwrap();
    let line = line.unwrap_or_else(|_| panic!("{point:?}: child never reported"));
    assert!(line.contains(MARKER), "{point:?}: {line}");
    use std::os::unix::process::ExitStatusExt;
    assert_eq!(
        status.signal(),
        Some(9),
        "{point:?}: child was not SIGKILLed"
    );
}

/// Applied heads the dead child recorded in each file's intent row.
fn intent_heads(files: &Files, index: usize) -> Option<String> {
    let path = files.root.join(format!("{}.db", FILES[index].0));
    let conn =
        rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .unwrap();
    conn.query_row(
        "SELECT heads FROM participant_intents WHERE batch_id='batch-kill'",
        [],
        |row| row.get(0),
    )
    .unwrap_or(None)
}

/// The tx-id vector an uncrashed run produces on an identical fresh fixture.
fn reference_heads() -> Vec<Vec<(String, i64)>> {
    let temp = tempfile::tempdir().unwrap();
    let files = files(temp.path());
    let (alpha, beta) = (open(&files, 0), open(&files, 1));
    let vector = as_writer(|| commit(&batch_for(&files, &alpha, &beta))).unwrap();
    vector.members.into_iter().map(|m| m.heads).collect()
}

fn heads(vector: &ReceiptVector) -> Vec<Vec<(String, i64)>> {
    vector.members.iter().map(|m| m.heads.clone()).collect()
}

#[test]
fn a_sigkill_before_the_decision_recovers_the_old_cut() {
    for point in [CrashPoint::AfterPrepare(0), CrashPoint::AfterPrepare(1)] {
        let temp = tempfile::tempdir().unwrap();
        let before = {
            let files = files(temp.path());
            (raw_head(&files, 0), raw_head(&files, 1))
        };
        kill_child_at(temp.path(), point);
        let files = describe(temp.path());
        let (alpha, beta) = (open(&files, 0), open(&files, 1));
        // The dead process's lease is gone; its intent still fences writes.
        let error = local_write(&files, &alpha, 0, "other").unwrap_err();
        assert!(
            error.to_string().contains("participant fenced"),
            "{point:?}: {error}"
        );
        assert_eq!(
            recover(&[&alpha, &beta], "batch-kill").unwrap(),
            Recovery::Aborted
        );
        assert_eq!(
            (raw_head(&files, 0), raw_head(&files, 1)),
            before,
            "{point:?}"
        );
        assert_eq!(counts(&alpha, 0).unwrap(), [0, 0]);
        assert_eq!(counts(&beta, 1).unwrap(), [0, 0]);
        let error = as_writer(|| commit(&batch_for(&files, &alpha, &beta))).unwrap_err();
        assert!(error.to_string().contains("aborted"), "{point:?}: {error}");
    }
}

#[test]
fn a_sigkill_after_the_decision_or_any_apply_recovers_the_reference_tx_vector() {
    let reference = reference_heads();
    for point in [
        CrashPoint::AfterDecision,
        CrashPoint::AfterApply(0),
        CrashPoint::AfterApply(1),
    ] {
        let temp = tempfile::tempdir().unwrap();
        drop(files(temp.path()));
        kill_child_at(temp.path(), point);
        let files = describe(temp.path());
        // What the dead child durably applied, read raw before recovery.
        let applied: Vec<_> = (0..2).map(|i| intent_heads(&files, i)).collect();
        let expected_applied = match point {
            CrashPoint::AfterDecision => 0,
            CrashPoint::AfterApply(i) => i + 1,
            _ => unreachable!(),
        };
        assert_eq!(
            applied.iter().filter(|h| h.is_some()).count(),
            expected_applied,
            "{point:?}: {applied:?}"
        );
        let (alpha, beta) = (open(&files, 0), open(&files, 1));
        let Recovery::Published(vector) = recover(&[&alpha, &beta], "batch-kill").unwrap() else {
            panic!("{point:?}: a durable decision recovered as an abort");
        };
        assert_eq!(heads(&vector), reference, "{point:?}");
        // A participant the child applied keeps exactly the child's tx ids.
        for (index, child) in applied.iter().enumerate() {
            if let Some(child) = child {
                let child: Vec<(String, i64)> = serde_json::from_str(child).unwrap();
                assert_eq!(child, vector.members[index].heads, "{point:?}");
            }
        }
        assert_eq!(counts(&alpha, 0).unwrap(), [1, 1]);
        assert_eq!(counts(&beta, 1).unwrap(), [1, 1]);
        let raw = (raw_head(&files, 0), raw_head(&files, 1));
        let retry = as_writer(|| commit(&batch_for(&files, &alpha, &beta))).unwrap();
        assert_eq!(retry, vector, "{point:?}");
        assert_eq!((raw_head(&files, 0), raw_head(&files, 1)), raw, "{point:?}");
    }
}
