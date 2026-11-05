use super::*;
use crate::search_outcome::observation::{Call, Surface};
use crate::search_outcome::{Operation, Outcome, Summary};
use std::os::unix::fs::{PermissionsExt, symlink};

fn directory() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    dir
}

fn event(surface: Surface, hit: bool) -> Event {
    Call::new(surface, Operation::Query)
        .finish(
            Some(200),
            Some(Summary {
                outcome: if hit { Outcome::Hits } else { Outcome::Empty },
                zero_kind: if hit {
                    None
                } else {
                    Some(crate::search_outcome::ZeroKind::EmptyRows)
                },
                candidate_count: None,
                admitted_count: None,
                result_count: Some(usize::from(hit)),
            }),
        )
        .unwrap()
}

#[test]
fn four_surfaces_persist_only_fixed_counts_across_connections() {
    let dir = directory();
    for surface in [
        Surface::Mcp,
        Surface::Http,
        Surface::CliLocal,
        Surface::Library,
    ] {
        let mut conn = open(dir.path()).unwrap();
        record(
            &mut conn,
            &crate::metrics::search_counter_delta(&event(surface, false)),
        )
        .unwrap();
        record(
            &mut conn,
            &crate::metrics::search_counter_delta(&event(surface, true)),
        )
        .unwrap();
        let failed = Call::new(surface, Operation::Query)
            .finish(Some(500), Some(Summary::unavailable(Outcome::Error)))
            .unwrap();
        record(&mut conn, &crate::metrics::search_counter_delta(&failed)).unwrap();
    }
    let conn = open(dir.path()).unwrap();
    let sum: i64 = conn
        .query_row(
            "SELECT sum(value) FROM counts WHERE series LIKE 'quipu_search_zero_results_total%'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(sum, 4); // hits and errors do not inflate misses
    let requests: i64 = conn
        .query_row(
            "SELECT sum(value) FROM counts WHERE series LIKE 'quipu_search_requests_total%'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(requests, 12);
    assert_eq!(
        conn.query_row(
            "SELECT count(*) FROM counts WHERE series LIKE '%logical_call%'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
}

#[test]
fn broad_or_symlinked_inventory_and_unknown_state_refuse() {
    let dir = directory();
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(open(dir.path()).is_err());
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let other = directory();
    symlink(
        other.path().join("victim"),
        dir.path().join("counts.sqlite"),
    )
    .unwrap();
    assert!(open(dir.path()).is_err());
    assert!(!other.path().join("victim").exists());
    std::fs::remove_file(dir.path().join("counts.sqlite")).unwrap();
    let conn = open(dir.path()).unwrap();
    conn.pragma_update(None, "user_version", 999).unwrap();
    assert!(open(dir.path()).is_err());
}

#[test]
fn child_privacy_boundary() {
    if std::env::var_os("QUIPU_COUNTER_TEST_CHILD").is_none() {
        return;
    }
    let mut value = event(Surface::Library, false);
    value.logical_call_id = Some("private-logical-sentinel".into());
    value.details = Some(crate::search_outcome::evidence::Details {
        context_reason: Some("private-query-sentinel".into()),
        ..Default::default()
    });
    super::super::observation::emit(&value);
}

#[test]
fn selected_mode_never_falls_back_to_query_journal_even_on_failure() {
    let dir = directory();
    let journal = directory();
    for valid in [true, false] {
        let destination = if valid {
            dir.path().to_path_buf()
        } else {
            dir.path().join("missing")
        };
        let result = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "search_outcome::counters_only::tests::child_privacy_boundary",
                "--nocapture",
            ])
            .env("QUIPU_COUNTER_TEST_CHILD", "1")
            .env("QUIPU_SEARCH_COUNTERS_ONLY_ENABLED", "true")
            .env("QUIPU_SEARCH_COUNTERS_ONLY_DIR", destination)
            .env("QUIPU_SEARCH_JOURNAL_DIR", journal.path())
            .output()
            .unwrap();
        assert!(result.status.success());
        assert_eq!(std::fs::read_dir(journal.path()).unwrap().count(), 0);
        if !valid {
            assert!(String::from_utf8_lossy(&result.stderr).contains("capture UNKNOWN"));
        }
    }
    let bytes = std::fs::read(dir.path().join("counts.sqlite")).unwrap();
    assert!(!bytes.windows(8).any(|s| s == b"sentinel"));
    let conn = open(dir.path()).unwrap();
    let n: i64 = conn
        .query_row(
            "SELECT sum(value) FROM counts WHERE series LIKE 'quipu_search_requests_total%'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(n, 1); // Actual emit, not merely the isolated recorder, reached state.
    let result = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "search_outcome::counters_only::tests::child_privacy_boundary",
        ])
        .env("QUIPU_COUNTER_TEST_CHILD", "1")
        .env("QUIPU_SEARCH_COUNTERS_ONLY_ENABLED", "false")
        .env("QUIPU_SEARCH_JOURNAL_DIR", journal.path())
        .output()
        .unwrap();
    assert!(result.status.success());
    let files = std::fs::read_dir(journal.path())
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(files.len(), 1); // Control proves the privacy test sees detailed capture.
    assert!(
        std::fs::read_to_string(files[0].path())
            .unwrap()
            .contains("private-query-sentinel")
    );
}
