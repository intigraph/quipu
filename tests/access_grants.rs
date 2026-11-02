//! Access-grant records (aegis-bqc): the shapes accept a well-formed grant and
//! its events, refuse each malformed or secret-carrying one, and the stored
//! query lists exactly the grants that are active now.
#![cfg(feature = "shacl")]

use std::collections::BTreeMap;

use quipu::types::Value;

const SHAPES: &str = include_str!("../shapes/access-grants.ttl");
const QUERY: &str = include_str!("../queries/active-access-grants.ttl");
const PREFIXES: &str = "@prefix a: <http://aegis.gastown.local/ontology/> .\n\
                        @prefix xsd: <http://www.w3.org/2001/XMLSchema#> .\n";
const DIGEST: &str = "sha256:0000000000000000000000000000000000000000000000000000000000000000";

/// A grant's fields, as `(predicate, object)`.
fn grant_fields(id: &str, expires: &str) -> Vec<(String, String)> {
    [
        ("a:grantId", format!("\"{id}\"")),
        ("a:grantPrincipal", "a:ellie".to_string()),
        ("a:aboutWorkItem", "a:aegis-bqc".to_string()),
        ("a:grantRole", "\"prometheus-read\"".to_string()),
        ("a:grantKind", "\"proxied\"".to_string()),
        ("a:policyDigest", format!("\"{DIGEST}\"")),
        (
            "a:issuedAt",
            "\"2026-11-01T00:00:00Z\"^^xsd:dateTime".to_string(),
        ),
        ("a:grantExpiresAt", format!("\"{expires}\"^^xsd:dateTime")),
        ("a:brokerKeyId", "\"broker-key-1\"".to_string()),
        ("a:attributionMode", "\"attribution\"".to_string()),
    ]
    .into_iter()
    .map(|(p, o)| (p.to_string(), o))
    .collect()
}

fn grant(id: &str, fields: &[(String, String)]) -> String {
    let mut out = format!("a:{id} a a:AccessGrant .\n");
    for (p, o) in fields {
        out.push_str(&format!("a:{id} {p} {o} .\n"));
    }
    out
}

fn event(id: &str, grant: &str, state: &str) -> String {
    format!(
        "a:{id} a a:AccessGrantEvent ; a:ofGrant a:{grant} ; a:grantState \"{state}\" ;\n\
         a:eventAt \"2026-11-01T00:00:01Z\"^^xsd:dateTime .\n"
    )
}

fn conforms(data: &str) -> bool {
    quipu::validate_shapes(SHAPES, &format!("{PREFIXES}{data}"))
        .unwrap()
        .conforms
}

#[test]
fn a_well_formed_grant_and_its_events_conform() {
    let data = format!(
        "{}{}{}",
        grant("g1", &grant_fields("g1", "2099-01-01T00:00:00Z")),
        event("e1", "g1", "intent"),
        event("e2", "g1", "committed")
    );
    assert!(conforms(&data));
}

#[test]
fn each_grant_field_is_required() {
    let fields = grant_fields("g1", "2099-01-01T00:00:00Z");
    for (i, (predicate, _)) in fields.iter().enumerate() {
        let mut fewer = fields.clone();
        fewer.remove(i);
        assert!(
            !conforms(&grant("g1", &fewer)),
            "missing {predicate} accepted"
        );
    }
}

#[test]
fn malformed_grants_are_refused() {
    let base = grant_fields("g1", "2099-01-01T00:00:00Z");
    let with = |predicate: &str, object: &str| {
        let mut fields = base.clone();
        for field in &mut fields {
            if field.0 == predicate {
                field.1 = object.to_string();
            }
        }
        grant("g1", &fields)
    };
    for (predicate, object) in [
        ("a:grantKind", "\"root\""),
        ("a:attributionMode", "\"trusted\""),
        ("a:policyDigest", "\"sha256:abc\""),
        ("a:grantPrincipal", "\"ellie\""),
        // Expiry before issue.
        ("a:grantExpiresAt", "\"2025-01-01T00:00:00Z\"^^xsd:dateTime"),
    ] {
        assert!(
            !conforms(&with(predicate, object)),
            "{predicate} {object} accepted"
        );
    }
}

#[test]
fn a_grant_or_event_cannot_carry_a_secret() {
    let mut fields = grant_fields("g1", "2099-01-01T00:00:00Z");
    fields.push((
        "a:capabilityToken".to_string(),
        "\"eyJhbGciOi...\"".to_string(),
    ));
    assert!(
        !conforms(&grant("g1", &fields)),
        "closed grant shape accepted a token"
    );

    let base = grant("g1", &grant_fields("g1", "2099-01-01T00:00:00Z"));
    let leaky = format!(
        "{base}a:e1 a a:AccessGrantEvent ; a:ofGrant a:g1 ; a:grantState \"delivered\" ;\n\
        a:eventAt \"2026-11-01T00:00:01Z\"^^xsd:dateTime ; a:clientSecret \"s3cr3t\" .\n"
    );
    assert!(!conforms(&leaky), "closed event shape accepted a secret");
}

#[test]
fn events_are_constrained() {
    let base = grant("g1", &grant_fields("g1", "2099-01-01T00:00:00Z"));
    assert!(
        !conforms(&format!("{base}{}", event("e1", "g1", "approved"))),
        "unknown state"
    );
    assert!(
        !conforms(&format!("{base}{}", event("e1", "nothing", "intent"))),
        "event of a non-grant"
    );
}

fn active(store: &quipu::Store, args: &[(&str, &str)]) -> Vec<String> {
    let query = store
        .query_get("active_access_grants")
        .unwrap()
        .expect("loaded");
    let args: BTreeMap<String, String> = args
        .iter()
        .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
        .collect();
    let sparql = query.render(&args).unwrap();
    let result = quipu::sparql::query(store, &sparql).unwrap();
    let mut ids: Vec<String> = result
        .rows()
        .iter()
        .map(|row| match row.get("grantId") {
            Some(Value::Str(s)) => s.clone(),
            other => format!("{other:?}"),
        })
        .collect();
    ids.sort();
    ids
}

#[test]
fn the_stored_query_lists_only_active_grants() {
    let mut store = quipu::Store::open_in_memory().unwrap();
    let mut data = String::from(PREFIXES);
    // active: committed, unexpired.
    data += &grant("live", &grant_fields("live", "2099-01-01T00:00:00Z"));
    data += &event("live-c", "live", "committed");
    data += &event("live-u", "live", "used");
    // revoked after commit.
    data += &grant("revoked", &grant_fields("revoked", "2099-01-01T00:00:00Z"));
    data += &event("revoked-c", "revoked", "committed");
    data += &event("revoked-r", "revoked", "revoked");
    // past its expiry, with no expired event yet.
    data += &grant("stale", &grant_fields("stale", "2026-11-01T00:00:05Z"));
    data += &event("stale-c", "stale", "committed");
    // intent only, never committed.
    data += &grant("pending", &grant_fields("pending", "2099-01-01T00:00:00Z"));
    data += &event("pending-i", "pending", "intent");
    // committed, for another principal and item.
    let mut other = grant_fields("other", "2099-01-01T00:00:00Z");
    other[1].1 = "a:kelly".to_string();
    other[2].1 = "a:aegis-ug3n".to_string();
    data += &grant("other", &other);
    data += &event("other-c", "other", "committed");
    assert!(quipu::validate_shapes(SHAPES, &data).unwrap().conforms);
    quipu::ingest_rdf(
        &mut store,
        data.as_bytes(),
        oxrdfio::RdfFormat::Turtle,
        None,
        "2026-11-01T00:00:00Z",
        None,
        None,
    )
    .unwrap();

    let shared = quipu::share_queries::from_turtle(QUERY).unwrap();
    assert_eq!(shared.len(), 1);
    store
        .query_load(&shared[0].query, "2026-11-01T00:00:00Z")
        .unwrap();

    assert_eq!(active(&store, &[]), ["live", "other"]);
    let ellie = "http://aegis.gastown.local/ontology/ellie";
    assert_eq!(active(&store, &[("principal", ellie)]), ["live"]);
    let ug3n = "http://aegis.gastown.local/ontology/aegis-ug3n";
    assert_eq!(active(&store, &[("item", ug3n)]), ["other"]);
}

#[test]
fn the_stored_query_file_is_in_canonical_form() {
    let shared = quipu::share_queries::from_turtle(QUERY).unwrap();
    assert_eq!(quipu::share_queries::to_turtle(&shared), QUERY);
}
