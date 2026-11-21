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

#[test]
fn scrape_renders_the_recorded_aggregate_with_fixed_families() {
    let dir = directory();
    let mut conn = open(dir.path()).unwrap();
    for (surface, hit) in [
        (Surface::Http, true),
        (Surface::Http, false),
        (Surface::CliLocal, false),
    ] {
        record(
            &mut conn,
            &crate::metrics::search_counter_delta(&event(surface, hit)),
        )
        .unwrap();
    }
    let text = aggregate(&open(dir.path()).unwrap()).unwrap();
    assert!(text.contains(
        "quipu_search_aggregate_requests_total{surface=\"http\",operation=\"query\",outcome=\"hits\"} 1\n"
    ));
    assert!(text.contains(
        "quipu_search_aggregate_zero_results_total{surface=\"cli_local\",operation=\"query\",kind=\"empty_rows\"} 1\n"
    ));
    assert!(text.contains("# TYPE quipu_search_aggregate_miss_reasons_total counter\n"));
    assert!(
        text.contains("quipu_search_aggregate_last_capture_timestamp_seconds{surface=\"http\"} ")
    );
    assert!(
        text.contains(
            "quipu_search_aggregate_last_capture_timestamp_seconds{surface=\"cli_local\"} "
        )
    );
    assert!(!text.contains("quipu_search_requests_total"));
    // Every sample line belongs to the fixed aggregate families.
    for line in text.lines().filter(|l| !l.starts_with('#')) {
        assert!(line.starts_with("quipu_search_aggregate_"), "{line}");
    }
}

#[test]
fn scrape_refuses_out_of_contract_state_whole() {
    let ok = "quipu_search_requests_total{surface=\"http\",operation=\"query\",outcome=\"hits\"}";
    let now = 1_000_000;
    assert!(render_rows(&[(ok.into(), 3, now)], now).is_ok());
    for (series, value, stamp) in [
        (
            "quipu_search_requests_total{surface=\"http\",operation=\"query\",outcome=\"hits\",query=\"x\"}",
            1,
            now,
        ),
        (
            "quipu_search_requests_total{surface=\"vati\",operation=\"query\",outcome=\"hits\"}",
            1,
            now,
        ),
        ("quipu_search_logical_call_total{}", 1, now),
        (ok, -1, now),
        (ok, 1, 0),
        (ok, 1, now + 6),
    ] {
        let rows = vec![(ok.to_string(), 1, now), (series.to_string(), value, stamp)];
        assert!(render_rows(&rows, now).is_err(), "{series} {value} {stamp}");
    }
    let many: Vec<_> = (0..4097).map(|_| (ok.to_string(), 1, now)).collect();
    assert!(render_rows(&many, now).is_err());
    // A text-typed count fails the strict column read, so nothing is published.
    let dir = directory();
    let conn = open(dir.path()).unwrap();
    conn.execute(
        "INSERT INTO counts(series,value,last_success) VALUES(?1,1,unixepoch())",
        [ok],
    )
    .unwrap();
    assert!(aggregate(&conn).is_ok());
    conn.execute_batch("PRAGMA ignore_check_constraints=ON; UPDATE counts SET value='seven';")
        .unwrap();
    assert!(aggregate(&conn).is_err());
}
