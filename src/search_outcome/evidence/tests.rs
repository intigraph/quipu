use super::*;
use serde_json::json;

#[test]
fn deeply_nested_or_wide_input_is_bounded_before_capture_serialization() {
    let mut nested = json!({"value":"private"});
    for _ in 0..32 {
        nested = json!({"nested":nested});
    }
    for input in [
        json!({"query":"device","filters":nested}),
        json!({"query":"device","filters":vec![0;5000]}),
    ] {
        assert!(!within_limits(&input));
        let details = Details::capture(&input, None, false);
        assert!(details.requested_options.is_none());
        assert_eq!(details.query_evidence.as_deref(), Some("device"));
    }
}

#[test]
fn private_evidence_preserves_safe_terms_but_scrubs_recognized_credentials() {
    for private in [
        "Bearer short-sensitive-value",
        "token=short-sensitive-value",
        "password='short-sensitive-value'",
        "sk-short-sensitive-value",
        "ghp_short_sensitive_value",
        "0123456789abcdef0123456789abcdef0123456789abcdef",
    ] {
        let details = Details::capture(
            &json!({"query":format!("device topology {private}")}),
            None,
            false,
        );
        let text = details.query_evidence.unwrap();
        assert!(text.starts_with("device topology "));
        assert!(text.contains("[REDACTED]"), "{private}: {text}");
        assert!(!text.contains(private));
        assert_eq!(details.query_redacted, Some(true));
    }
    let details = Details::capture(&json!({"query":"device topology"}), None, false);
    assert_eq!(details.query_evidence.as_deref(), Some("device topology"));
    assert_eq!(details.query_redacted, Some(false));
}

#[test]
fn oversized_or_unobserved_metadata_is_explicitly_absent() {
    let details = Details::capture(&json!({"query":"x".repeat(4097)}), None, false);
    assert_eq!(details.query_bytes, Some(4097));
    assert!(details.query_evidence.is_none());
    assert!(details.query_redacted.is_none());
    assert!(details.index_revision.is_none());
    assert!(details.config_revision.is_none());
    assert!(details.resolved_scope.is_none());
    let serialized = serde_json::to_value(details).unwrap();
    assert!(serialized["index_revision"].is_null());
}

#[test]
fn options_and_response_reason_do_not_invent_context_on_unknown_payloads() {
    let input = json!({"query":"device", "graph":"urn:example:graph", "limit":5,
        "params":{"token":"never-retain-param"},"embedding":[1,2],
        "filters":{"token":"never-retain-filter"}});
    let response = json!({"search_scope":{"mode":"root"}, "context":{"reason":"no_candidates"}});
    let details = Details::capture(&input, Some(&response), false);
    assert!(details.context_reason.is_none());
    assert_eq!(details.resolved_scope, Some(json!({"mode":"root"})));
    let text = serde_json::to_string(&details).unwrap();
    assert!(!text.contains("never-retain-param"));
    assert!(!text.contains("never-retain-filter"));
    assert!(!text.contains("embedding"));
    assert!(text.contains("urn:example:graph"));
    assert_eq!(
        Details::capture(&input, Some(&response), true)
            .context_reason
            .as_deref(),
        Some("no_candidates")
    );
}
