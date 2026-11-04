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

/// aegis-7zp4rc.2: a verified attestation supplies agent and session, so a
/// signed writer that (correctly) omits those headers is complete when the
/// rest of its declaration is.
#[test]
fn a_verified_identity_supplies_agent_and_session_only() {
    // The sd signed-write shape: harness, host and model, no agent/session.
    let signed = class([None, Some("claude"), Some("host-a"), None, Some("m")]);
    assert_eq!(signed.completeness, Completeness::Partial);
    assert_eq!(signed.missing, vec!["agent", "session"]);
    let credited = signed.with_verified_identity();
    assert_eq!(credited.completeness, Completeness::Complete);
    assert!(credited.missing.is_empty());
    assert_eq!(
        (credited.client.as_str(), credited.endpoint.as_str()),
        (signed.client.as_str(), signed.endpoint.as_str())
    );

    // Negative control: the key never supplies host, harness or model.
    let no_model = class([None, Some("codex"), Some("host-a"), None, None]);
    let credited = no_model.with_verified_identity();
    assert_eq!(credited.completeness, Completeness::Partial);
    assert_eq!(credited.missing, vec!["model"]);
    let bare = class([None, None, None, None, None]).with_verified_identity();
    assert_eq!(bare.completeness, Completeness::Partial);
    assert_eq!(bare.missing, vec!["harness", "host"]);

    // Already complete stays complete.
    let full = class([Some("a"), Some("cron"), Some("h"), None, None]);
    assert_eq!(full.with_verified_identity(), full);
}
