//! Real scoped HTTP minting; fixture credentials and in-memory stores only.
use super::*;
use quipu::{entity_namespaces::Namespace, writer_scope::MintScope};

const KIND: &str = "urn:fixture:Host";
const OTHER: &str = "urn:fixture:Document";
const PREFIX: &str = "fixture-infra";
const BASE: &str = "https://example.org/infra/id/";
const GRAPH: &str = "urn:g:a";

fn fixture() -> (quipu::Store, WriteGrant) {
    let mut store = quipu::Store::open_in_memory().unwrap();
    store.graph_create(GRAPH).unwrap();
    store.graph_create("urn:g:b").unwrap();
    let mut config = quipu::QuipuConfig::default();
    for (prefix, base) in [(PREFIX, BASE), ("foreign", "https://example.org/foreign/")] {
        config.namespaces.push(Namespace {
            prefix: prefix.into(),
            base_iri: base.into(),
            minting: "key".into(),
            types: vec![KIND.into(), OTHER.into()],
            graphs: vec![GRAPH.into(), "urn:g:b".into()],
            ..Namespace::default()
        });
    }
    quipu::entity_namespaces::configure(&mut store, &config).unwrap();
    quipu::entity_namespaces::apply(&mut store).unwrap();
    let shapes = format!(
        "@prefix sh: <http://www.w3.org/ns/shacl#> . \
         @prefix qp: <https://quipu.dev/ontology/> . \
         <urn:shape:host> a sh:NodeShape; sh:targetClass <{KIND}>; qp:keyFields (<urn:key:asset>); \
         sh:property [sh:path <urn:key:asset>; sh:datatype <http://www.w3.org/2001/XMLSchema#string>; sh:minCount 1; sh:maxCount 1] . \
         <urn:shape:document> a sh:NodeShape; sh:targetClass <{OTHER}>; qp:keyFields (<urn:key:asset>); \
         sh:property [sh:path <urn:key:asset>; sh:datatype <http://www.w3.org/2001/XMLSchema#string>; sh:minCount 1; sh:maxCount 1] ."
    );
    quipu::mcp::tools::tool_shapes(
        &mut store,
        &json!({"action":"load","name":"mint-keys","turtle":shapes}),
    )
    .unwrap();
    let mut enrollment = grant(&store, "alice", GRAPH);
    enrollment.operations = BTreeSet::from([WriteOperation::EntityRefMint]);
    enrollment.mint_scope = Some(MintScope {
        namespace: PREFIX.into(),
        base_iri: BASE.into(),
        types: BTreeSet::from([KIND.into()]),
    });
    (store, enrollment)
}

#[tokio::test]
async fn scoped_mint_exact_selectors_default_off_and_no_authority_fallback() {
    let (store, mut enrollment) = fixture();
    let shared = Arc::new(crate::StoreHandle::writer_only(store));
    let policy = BearerPolicy::new(Some("fixture-operator".into()), None, None, 0)
        .unwrap()
        .with_scoped_writers(registry())
        .unwrap();
    let router = Router::new()
        .route("/entity_ref", post(crate::tools::entity_ref))
        .route("/update", post(crate::update::update_post))
        .with_state(shared.clone())
        .layer(middleware::from_fn(
            move |req: axum::extract::Request, next| {
                let policy = policy.clone();
                async move {
                    let (header, authorization) =
                        super::super::authorize_request(&req, &policy, false);
                    if authorization.decision != AccessDecision::Allow {
                        return (axum::http::StatusCode::UNAUTHORIZED, "invalid credential")
                            .into_response();
                    }
                    super::super::run_authorized(
                        req,
                        next,
                        &policy,
                        authorization,
                        header.as_deref(),
                    )
                    .await
                }
            },
        ));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap().to_string();
    let serving = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    tokio::task::spawn_blocking(move || {
        let input = json!({"action":"mint","namespace":PREFIX,"type":KIND,"graph":GRAPH,
            "keys":{"urn:key:asset":"fixture-asset"},"label":"worker"});
        let initial = shared.lock().latest_tx_id().unwrap();
        // Registered namespace and valid named token alone grant nothing.
        assert_eq!(
            request(
                &address,
                "alice",
                "/entity_ref",
                &input.to_string(),
                "application/json"
            )
            .0,
            403
        );
        assert_eq!(shared.lock().latest_tx_id().unwrap(), initial);
        shared.lock().writer_grant_apply(&enrollment).unwrap();
        let (status, accepted) = request(
            &address,
            "alice",
            "/entity_ref",
            &input.to_string(),
            "application/json",
        );
        assert_eq!(status, 200, "{accepted}");
        assert!(accepted["iri"].as_str().unwrap().starts_with(BASE));
        println!("mint response: {accepted}");
        let tx = accepted["tx_id"].as_i64().unwrap();
        let evidence = shared.lock().transaction_auth(tx).unwrap().unwrap();
        assert_eq!(evidence.principal, "urn:writer:alice");
        assert_eq!(evidence.credential_id.as_deref(), Some("alice"));
        assert_eq!(evidence.auth_class, "scoped_named_bearer");
        let (status, repeat) = request(
            &address,
            "alice",
            "/entity_ref",
            &input.to_string(),
            "application/json",
        );
        assert_eq!(status, 200, "{repeat}");
        assert_eq!(repeat["converged"], true);
        assert!(repeat["tx_id"].is_null());
        assert_eq!(repeat["iri"], accepted["iri"]);
        println!("repeat response: {repeat}");
        let mut before = shared.lock().latest_tx_id().unwrap();
        let mut variants = Vec::new();
        for (field, value) in [
            ("graph", json!("urn:g:b")),
            ("type", json!(OTHER)),
            ("namespace", json!("foreign")),
            ("action", json!("resolve")),
            ("action", json!("verify")),
        ] {
            let mut denied = input.clone();
            denied[field] = value;
            variants.push(denied);
        }
        let mut absent = input.clone();
        absent.as_object_mut().unwrap().remove("graph");
        variants.push(absent);
        for denied in variants {
            let (status, report) = request(
                &address,
                "alice",
                "/entity_ref",
                &denied.to_string(),
                "application/json",
            );
            assert_eq!(status, 403, "{denied}: {report}");
            assert_eq!(shared.lock().latest_tx_id().unwrap(), before);
        }
        assert_eq!(
            request(
                &address,
                "bob",
                "/entity_ref",
                &input.to_string(),
                "application/json"
            )
            .0,
            403
        );
        // A mint-only grant cannot turn into a general graph writer.
        assert_eq!(
            request(
                &address,
                "alice",
                "/update",
                "INSERT DATA { GRAPH <urn:g:a> { <urn:never-interned> <urn:p> 1 } }",
                "application/sparql-update"
            )
            .0,
            403
        );
        assert_eq!(shared.lock().lookup("urn:never-interned").unwrap(), None);
        assert_eq!(shared.lock().latest_tx_id().unwrap(), before);
        // Separate narrow grants compose without depending on policy row order.
        let mut document_grant = enrollment.clone();
        document_grant.id = "alice-documents".into();
        document_grant.mint_scope.as_mut().unwrap().types = BTreeSet::from([OTHER.into()]);
        shared.lock().writer_grant_apply(&document_grant).unwrap();
        let mut document = input.clone();
        document["type"] = json!(OTHER);
        let (status, report) = request(
            &address,
            "alice",
            "/entity_ref",
            &document.to_string(),
            "application/json",
        );
        assert_eq!(status, 200, "{report}");
        assert_ne!(report["iri"], accepted["iri"]);
        before = shared.lock().latest_tx_id().unwrap();
        // Prefix rebinding cannot redirect an old grant to another base.
        enrollment.revision = 2;
        enrollment.mint_scope.as_mut().unwrap().base_iri = "https://example.org/wrong/".into();
        shared.lock().writer_grant_apply(&enrollment).unwrap();
        assert_eq!(
            request(
                &address,
                "alice",
                "/entity_ref",
                &input.to_string(),
                "application/json"
            )
            .0,
            403
        );
        enrollment.revision = 3;
        enrollment.mint_scope.as_mut().unwrap().base_iri = BASE.into();
        shared.lock().writer_grant_apply(&enrollment).unwrap();
        assert_eq!(
            request(
                &address,
                "alice",
                "/entity_ref",
                &input.to_string(),
                "application/json"
            )
            .0,
            200
        );
        enrollment.revision = 4;
        enrollment.revoked = true;
        shared.lock().writer_grant_apply(&enrollment).unwrap();
        assert_eq!(
            request(
                &address,
                "alice",
                "/entity_ref",
                &input.to_string(),
                "application/json"
            )
            .0,
            403
        );
        assert_eq!(shared.lock().latest_tx_id().unwrap(), before);
        enrollment.revision = 5;
        enrollment.revoked = false;
        enrollment.valid_until = 1;
        shared.lock().writer_grant_apply(&enrollment).unwrap();
        assert_eq!(
            request(
                &address,
                "alice",
                "/entity_ref",
                &input.to_string(),
                "application/json"
            )
            .0,
            403
        );
        assert_eq!(shared.lock().latest_tx_id().unwrap(), before);
    })
    .await
    .unwrap();
    serving.abort();
}

#[test]
fn mint_enrollment_requires_exact_nonempty_scope_and_mint_only_operation() {
    let (store, enrollment) = fixture();
    for mutation in 0..7 {
        let mut invalid = enrollment.clone();
        match mutation {
            0 => invalid.mint_scope = None,
            1 => invalid.mint_scope.as_mut().unwrap().types.clear(),
            2 => invalid.mint_scope.as_mut().unwrap().namespace.clear(),
            3 => invalid.mint_scope.as_mut().unwrap().base_iri = "relative/".into(),
            4 => {
                invalid.operations.insert(WriteOperation::Update);
            }
            5 => invalid.operations = BTreeSet::from([WriteOperation::Update]),
            _ => invalid.mint_scope.as_mut().unwrap().types = BTreeSet::from(["*".into()]),
        }
        assert!(store.writer_grant_apply(&invalid).is_err());
    }
}
