//! Real HTTP adapters, synthetic credentials and disposable stores only.
#[path = "scoped_writer_mint_tests.rs"]
mod mint;
use std::{collections::BTreeSet, sync::Arc};

use axum::{Router, middleware, response::IntoResponse, routing::post};
use quipu::{
    Store,
    crew_credentials::CredentialRegistry,
    http_auth::{AccessDecision, BearerPolicy},
    writer_scope::{WriteGrant, WriteOperation},
};
use serde_json::json;
use sha2::{Digest, Sha256};

fn registry() -> CredentialRegistry {
    CredentialRegistry::parse(&serde_json::to_vec(&json!({"version":1,"credentials": [
        {"credential_id":"alice","principal":"urn:writer:alice","audience":"quipu","token_sha256":format!("{:x}",Sha256::digest(b"fixture-alice"))},
        {"credential_id":"bob","principal":"urn:writer:bob","audience":"quipu","token_sha256":format!("{:x}",Sha256::digest(b"fixture-bob"))}
    ]})).unwrap(), "quipu").unwrap()
}

fn grant(store: &Store, who: &str, graph: &str) -> WriteGrant {
    WriteGrant {
        version: 1,
        id: who.into(),
        principal: format!("urn:writer:{who}"),
        credential_id: who.into(),
        instance: store.writer_graph_instance(graph).unwrap(),
        operations: BTreeSet::from([
            WriteOperation::Update,
            WriteOperation::Knot,
            WriteOperation::Episode,
        ]),
        valid_from: 0,
        valid_until: quipu::time::epoch_secs() + 3600,
        revision: 1,
        revoked: false,
        mint_scope: None,
    }
}

fn request(
    address: &str,
    who: &str,
    path: &str,
    body: &str,
    content_type: &str,
) -> (u16, serde_json::Value) {
    request_with_write_id(address, who, path, body, content_type, None)
}

fn request_with_write_id(
    address: &str,
    who: &str,
    path: &str,
    body: &str,
    content_type: &str,
    write_id: Option<&str>,
) -> (u16, serde_json::Value) {
    let mut request = ureq::post(&format!("http://{address}{path}"))
        .set("authorization", &format!("Bearer fixture-{who}"))
        .set("content-type", content_type)
        .set("x-quipu-client", "agent-adhoc")
        .set("x-quipu-agent", "urn:writer:bob")
        .set("x-quipu-principal", "urn:writer:bob");
    if let Some(write_id) = write_id {
        request = request.set("x-quipu-write-id", write_id);
    }
    let result = request.send_string(body);
    let response = match result {
        Ok(r) | Err(ureq::Error::Status(_, r)) => r,
        Err(e) => panic!("{e}"),
    };
    let status = response.status();
    let text = response.into_string().unwrap();
    println!("fixture request: principal={who}; path={path}; status={status}");
    (
        status,
        serde_json::from_str(&text).unwrap_or(json!({"text":text})),
    )
}

#[tokio::test]
async fn scoped_writer_http_two_way_scope_and_refusal_atomicity() {
    for authority in [false, true] {
        let mut store = Store::open_in_memory().unwrap();
        store.set_base_ns("https://example.org/ontology/");
        for (who, graph) in [("alice", "urn:g:a"), ("bob", "urn:g:b")] {
            store.graph_create(graph).unwrap();
            store
                .writer_grant_apply(&grant(&store, who, graph))
                .unwrap();
        }
        store.governance_config_mut().enforce_authority = authority;
        let shared = Arc::new(crate::StoreHandle::writer_only(store));
        let policy = BearerPolicy::new(Some("fixture-operator".into()), None, None, 0)
            .unwrap()
            .with_scoped_writers(registry())
            .unwrap();
        let router = Router::new()
            .route("/update", post(crate::update::update_post))
            .route("/knot", post(crate::publication::knot))
            .route("/knot/status", post(crate::knot_completion::status))
            .route("/episode", post(crate::tools::episode))
            .route("/set", post(crate::tools::set_predicate))
            .route("/graph/create", post(crate::tools::graph_create))
            .with_state(shared.clone())
            .layer(middleware::from_fn(
                move |req: axum::extract::Request, next| {
                    let policy = policy.clone();
                    async move {
                        let (header, authorization) =
                            super::authorize_request(&req, &policy, false);
                        if authorization.decision != AccessDecision::Allow {
                            return (axum::http::StatusCode::UNAUTHORIZED, "invalid credential")
                                .into_response();
                        }
                        super::run_authorized(req, next, &policy, authorization, header.as_deref())
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
            let content_type = "application/sparql-update";
            for (who,allowed,foreign) in [("alice","urn:g:a","urn:g:b"),("bob","urn:g:b","urn:g:a")] {
                let update = format!("INSERT {{ GRAPH <{allowed}> {{ <urn:subject:{who}> <urn:p> 1 }} }} WHERE {{ FILTER NOT EXISTS {{ GRAPH <{allowed}> {{ <urn:subject:{who}> <urn:p> ?old }} }} }}");
                let (status,report) = request(&address,who,"/update?actor=urn:writer:impostor",&update,content_type);
                assert_eq!(status,200,"{report}");
                assert_eq!(report["asserted"],1);
                assert_eq!(report["retracted"],0);
                let tx = report["tx"].as_i64().unwrap();
                let evidence = shared.lock().transaction_auth(tx).unwrap().unwrap();
                assert_eq!(evidence.principal,format!("urn:writer:{who}"));
                assert_eq!(evidence.credential_id.as_deref(),Some(who));
                assert_eq!(evidence.auth_class,"scoped_named_bearer");
                // Lost response: retry the SAME conditional request. It must
                // report no change, not silently duplicate an accepted write.
                let (status,retry) = request(&address,who,"/update",&update,content_type);
                assert_eq!(status,200,"{retry}");
                assert_eq!(retry["asserted"],0); assert_eq!(retry["retracted"],0); assert!(retry["tx"].is_null());
                let knot = json!({"graph":allowed,"turtle":format!("<urn:knot:{who}> <urn:p> 4 ."),"source":format!("snapshot:{who}"),"snapshot":format!("snapshot:{who}"),"replace_snapshot":true});
                let (status,knot_report) = request(&address,who,"/knot",&knot.to_string(),"application/json");
                assert_eq!(status,200,"{knot_report}");
                // The completion wrapper must retain the scoped writer barrier.
                // A foreign knot records STOPPED, never an accepted transaction.
                for (target, expected_status, expected_state) in [(allowed, 200, "finished"), (foreign, 403, "stopped")] {
                    let input = json!({"graph":target,"turtle":format!("<urn:completion:{who}> <urn:p> 5 .")});
                    let write_id = format!("{:x}", Sha256::digest(format!("{who}:{target}")));
                    let before = shared.lock().latest_tx_id().unwrap();
                    let (status, _) = request_with_write_id(&address,who,"/knot",&input.to_string(),"application/json",Some(&write_id));
                    assert_eq!(status,expected_status);
                    let query = json!({"write_id":write_id,"request":input});
                    let (status, completion) = request(&address,who,"/knot/status",&query.to_string(),"application/json");
                    assert_eq!(status,200);
                    assert_eq!(completion["state"],expected_state);
                    if expected_status == 403 {
                        assert_eq!(shared.lock().latest_tx_id().unwrap(),before);
                        assert!(completion.get("receipt").is_none());
                    } else {
                        assert!(completion["receipt"]["tx_id"].is_u64());
                    }
                    assert_eq!(request_with_write_id(&address,who,"/knot",&input.to_string(),"application/json",Some(&write_id)).0,400);
                }
                let ep = json!({"graph":allowed,"name":format!("scoped-{who}"),"episode_body":"scoped fixture","nodes":[],"edges":[]});
                let (status,ep_report) = request(&address,who,"/episode",&ep.to_string(),"application/json");
                assert_eq!(status,200,"{ep_report}");
                let before = shared.lock().latest_tx_id().unwrap();
                let graphs = shared.lock().all_named_graph_ids().unwrap();
                for denied in [
                    format!("INSERT DATA {{ GRAPH <{foreign}> {{ <urn:never-interned> <urn:never-p> 2 }} }}"),
                    format!("INSERT DATA {{ GRAPH <{allowed}> {{ <urn:never-interned> <urn:never-p> 2 }} GRAPH <{foreign}> {{ <urn:never-interned> <urn:never-p> 3 }} }}"),
                    format!("INSERT {{ GRAPH <{foreign}> {{ <urn:never-interned> <urn:never-p> 2 }} }} WHERE {{ FILTER(false) }}"),
                    "INSERT DATA { <urn:never-interned> <urn:never-p> 2 }".into(),
                    format!("INSERT {{ GRAPH ?g {{ <urn:never-interned> <urn:never-p> 2 }} }} WHERE {{ VALUES ?g {{ <{allowed}> }} }}"),
                    format!("CLEAR GRAPH <{allowed}>"),
                    format!("CREATE GRAPH <{allowed}>"),
                ] {
                    let (status,out) = request(&address,who,"/update",&denied,content_type);
                    assert_eq!(status,403,"{denied}: {out}");
                    assert_eq!(shared.lock().latest_tx_id().unwrap(),before);
                    assert_eq!(shared.lock().lookup("urn:never-interned").unwrap(),None);
                    assert_eq!(shared.lock().lookup("urn:never-p").unwrap(),None);
                    assert_eq!(shared.lock().all_named_graph_ids().unwrap(),graphs);
                }
                for (route,body) in [
                    ("/knot",json!({"graph":foreign,"turtle":"<urn:never-interned> <urn:never-p> 1 .","replace_snapshot":true,"source":"foreign-snapshot"})),
                    ("/episode",json!({"graph":foreign,"name":"never-episode","nodes":[],"replace_snapshot":true})),
                    ("/graph/create",json!({"graph":"urn:never-created"})),
                    ("/set",json!({"entity":format!("urn:subject:{who}"),"predicate":"urn:p","value":9})),
                    ("/set",json!({"graph":allowed,"entity":format!("urn:subject:{who}"),"predicate":"urn:p","value":9})),
                ] {
                    let (status,out) = request(&address,who,route,&body.to_string(),"application/json");
                    assert_eq!(status,403,"{route}: {out}");
                    assert_eq!(shared.lock().latest_tx_id().unwrap(),before);
                    assert_eq!(shared.lock().all_named_graph_ids().unwrap(),graphs);
                }
                assert_eq!(request(&address,"invalid","/update",&update,content_type).0,401);
            }
            // A completed owner-local withdrawal is visible to the next
            // request without restarting or reloading the verifier registry.
            let mut revoke = grant(&shared.lock(),"alice","urn:g:a");
            revoke.revision = 2; revoke.revoked = true;
            shared.lock().writer_grant_apply(&revoke).unwrap();
            assert_eq!(request(&address,"alice","/update","INSERT DATA { GRAPH <urn:g:a> { <urn:denied> <urn:p> 9 } }",content_type).0,403);
        }).await.unwrap();
        serving.abort();
    }
}

#[test]
fn scoped_verifiers_cannot_overlap_shared_named_or_replication_authority() {
    let same = registry();
    assert!(
        BearerPolicy::new(Some("fixture-alice".into()), None, None, 0)
            .unwrap()
            .with_scoped_writers(same.clone())
            .is_err()
    );
    assert!(
        BearerPolicy::new(Some("other".into()), None, None, 0)
            .unwrap()
            .with_named(same.clone())
            .with_scoped_writers(same.clone())
            .is_err()
    );
    assert!(
        BearerPolicy::new(Some("other".into()), None, None, 0)
            .unwrap()
            .with_replication_only(same.clone())
            .unwrap()
            .with_scoped_writers(same)
            .is_err()
    );
}
