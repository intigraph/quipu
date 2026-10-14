use super::*;

fn class(values: [Option<&str>; 5]) -> RequestProvenance {
    RequestProvenance::classify("agent-adhoc", "/episode", values)
}

#[test]
fn a_producer_naming_itself_is_complete_without_session_or_model() {
    let p = class([
        Some("write-probe"),
        Some("service"),
        Some("host-b"),
        None,
        None,
    ]);
    assert_eq!(p.completeness, Completeness::Complete);
    assert!(p.missing.is_empty());
}

#[test]
fn an_agent_harness_also_needs_session_and_model() {
    let p = class([
        Some("ian"),
        Some("claude"),
        Some("host-a"),
        Some("s-1"),
        None,
    ]);
    assert_eq!(p.completeness, Completeness::Partial);
    assert_eq!(p.missing, vec!["model"]);
    let full = class([
        Some("ian"),
        Some("Codex"),
        Some("host-a"),
        Some("s-1"),
        Some("m"),
    ]);
    assert_eq!(full.completeness, Completeness::Complete);
}

#[test]
fn no_header_is_absent_and_reports_every_required_field() {
    let p = class([None, None, None, None, None]);
    assert_eq!(p.completeness, Completeness::Absent);
    assert_eq!(p.missing, vec!["agent", "harness", "host"]);
}

#[test]
fn a_blank_header_is_not_a_declaration() {
    let p = class([Some("  "), Some("\t"), None, None, None]);
    assert_eq!(p.completeness, Completeness::Absent);
    let q = class([Some("ian"), Some(" "), Some("host-a"), None, None]);
    assert_eq!(q.completeness, Completeness::Partial);
    assert_eq!(q.missing, vec!["harness"]);
}

#[test]
fn scope_carries_and_restores() {
    assert!(current().is_none());
    let p = Arc::new(class([Some("a"), Some("cron"), Some("h"), None, None]));
    scoped(Some(p.clone()), || {
        assert_eq!(current().as_deref(), Some(&*p));
        // A nested None keeps the outer scope, like write_kind.
        scoped(None, || assert_eq!(current().as_deref(), Some(&*p)));
    });
    assert!(current().is_none());
}

#[test]
fn committed_declarations_are_atomic_bounded_and_not_authentication() {
    let mut store = crate::Store::open_in_memory().unwrap();
    let p = Arc::new(class([
        Some("actor"),
        Some("service"),
        Some("host-a"),
        None,
        None,
    ]));
    let tx = scoped(Some(p), || {
        store.transact(&[], "2026-01-01T00:00:00Z", Some("different"), None)
    })
    .unwrap();
    assert_eq!(
        store.transaction_declarations(tx).unwrap().unwrap()["host"],
        "host-a"
    );
    assert!(store.transaction_auth(tx).unwrap().is_none());
    let plain = store
        .transact(&[], "2026-01-01T00:00:00Z", None, None)
        .unwrap();
    assert!(store.transaction_declarations(plain).unwrap().is_none());
    store
        .conn
        .execute_batch("SAVEPOINT declared_rollback")
        .unwrap();
    let tx = scoped(
        Some(Arc::new(class([None, None, Some("host-b"), None, None]))),
        || store.transact(&[], "2026-01-01T00:00:00Z", None, None),
    )
    .unwrap();
    assert!(store.transaction_declarations(tx).unwrap().is_some());
    store
        .conn
        .execute_batch("ROLLBACK TO declared_rollback; RELEASE declared_rollback")
        .unwrap();
    assert!(store.transaction_declarations(tx).unwrap().is_none());
    let oversized = "x".repeat(1025);
    assert!(
        !class([None, None, Some(&oversized), None, None])
            .declarations
            .contains_key("host")
    );
}
