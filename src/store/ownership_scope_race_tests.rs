use super::*;
use std::sync::mpsc;
use std::time::Duration;

/// These pauses are inside the actual callback / after the actual SQL call,
/// not a reimplementation of admission. Both producers have bounded waits.
#[test]
fn retirement_cannot_cross_the_callback_or_physical_sql_completion_boundary() {
    for after_sql in [false, true] {
        let f = Fixture::new();
        let active = f.active();
        let retiring = f.retiring();
        let journal = f.journal.clone();
        let database = f.path.clone();
        let fence = Arc::clone(f.store.ownership_fence.as_ref().unwrap());
        let (arrived, arrival) = mpsc::channel();
        let (release, resume) = mpsc::channel();
        let pause: Box<dyn Fn() + Send> = Box::new(move || {
            arrived.send(()).unwrap();
            resume.recv_timeout(Duration::from_secs(5)).unwrap();
        });
        if after_sql {
            *fence.after_sql.lock().unwrap() = Some(pause);
        } else {
            *fence.before_commit.lock().unwrap() = Some(pause);
        }
        let mut store = f.store;
        let writer = std::thread::spawn(move || {
            let result = store.ownership_transaction(|s| {
                s.conn
                    .execute("INSERT INTO scope_probe VALUES ('writer')", [])?;
                Ok(())
            });
            (store, result)
        });
        let reached = arrival.recv_timeout(Duration::from_secs(5));
        let transition =
            super::super::super::ownership_journal::advance(&journal, &active, &retiring);
        let visible = if reached.is_ok() && after_sql {
            let reader = rusqlite::Connection::open(&database).unwrap();
            reader
                .query_row(
                    "SELECT count(*) FROM scope_probe WHERE value='writer'",
                    [],
                    |r| r.get::<_, i64>(0),
                )
                .unwrap()
        } else {
            0
        };
        let _ = release.send(());
        let (mut store, result) = writer.join().unwrap();
        *fence.before_commit.lock().unwrap() = None;
        *fence.after_sql.lock().unwrap() = None;
        assert!(reached.is_ok(), "real SQL boundary was not reached");
        result.unwrap();
        assert!(
            transition.is_err(),
            "retirement crossed the admitted commit boundary; after_sql={after_sql}"
        );
        if after_sql {
            assert_eq!(
                visible, 1,
                "row durable before the shared permit is released"
            );
        }
        super::super::super::ownership_journal::advance(&journal, &active, &retiring).unwrap();
        let before = snapshot(&store);
        assert!(
            store
                .ownership_transaction(|s| write(s, "after-retirement"))
                .is_err()
        );
        assert_eq!(snapshot(&store), before);
    }
}

#[test]
fn exclusive_transition_blocks_new_scopes_and_temporary_contention_is_not_admission() {
    let mut f = Fixture::new();
    let token = ownership_latch::exclusive(&f.journal).unwrap();
    let before = snapshot(&f.store);
    assert!(
        f.store
            .ownership_transaction(|s| write(s, "blocked"))
            .is_err()
    );
    assert_eq!(snapshot(&f.store), before);
    drop(token);
    f.store
        .ownership_transaction(|s| write(s, "positive"))
        .unwrap();
    super::super::super::ownership_journal::advance(&f.journal, &f.active(), &f.retiring())
        .unwrap();
    assert!(f.store.ownership_operation(|s| write(s, "stale")).is_err());
}

#[cfg(unix)]
#[test]
fn an_independent_process_exclusive_latch_refuses_the_actual_store_scope() {
    use std::io::{BufRead, Write};
    use std::process::{Child, Command, Stdio};
    struct OwnedChild(Child);
    impl Drop for OwnedChild {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let mut f = Fixture::new();
    let lock = ownership_latch::sibling(&f.journal, ".lock").unwrap();
    let script = "import fcntl,sys\nf=open(sys.argv[1],'r+')\nfcntl.flock(f,fcntl.LOCK_EX|fcntl.LOCK_NB)\nprint('LOCKED',flush=True)\nsys.stdin.readline()\n";
    let mut child = OwnedChild(
        Command::new("python3")
            .args(["-c", script])
            .arg(&lock)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let stdout = child.0.stdout.take().unwrap();
    let (send, ready) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        let mut line = String::new();
        let result = std::io::BufReader::new(stdout).read_line(&mut line);
        let _ = send.send((result, line));
    });
    let (_, line) = ready.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(line.trim(), "LOCKED");
    let before = snapshot(&f.store);
    assert!(
        f.store
            .ownership_transaction(|s| write(s, "external-lock"))
            .is_err()
    );
    assert_eq!(snapshot(&f.store), before);
    writeln!(child.0.stdin.take().unwrap(), "release").unwrap();
    let end = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(status) = child.0.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        assert!(
            std::time::Instant::now() < end,
            "owned fixture child deadline"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    reader.join().unwrap();
    f.store
        .ownership_transaction(|s| write(s, "after-external-lock"))
        .unwrap();
}
