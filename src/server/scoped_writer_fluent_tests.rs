//! Scoped machine writers may append fluent observations to their granted
//! graph only (aegis-karb.8.1). Real HTTP adapter, synthetic credentials and a
//! disposable store.
use std::{collections::BTreeSet, sync::Arc};

use axum::{Router, middleware, response::IntoResponse, routing::post};
use quipu::{
    Store,
    http_auth::{AccessDecision, BearerPolicy},
    writer_scope::{WriteGrant, WriteOperation},
};
use serde_json::json;

use super::{registry, request};

const AT: &str = "2026-11-19T00:00:00Z";

fn grant(store: &Store, who: &str, graph: &str, operations: &[WriteOperation]) -> WriteGrant {
    WriteGrant {
        version: 1,
        id: who.into(),
        principal: format!("urn:writer:{who}"),
        credential_id: who.into(),
        instance: store.writer_graph_instance(graph).unwrap(),
        operations: operations.iter().copied().collect::<BTreeSet<_>>(),
        valid_from: 0,
        valid_until: quipu::time::epoch_secs() + 3600,
        revision: 1,
        revoked: false,
        mint_scope: None,
    }
}

fn observation(graph: &str, event: &str, value: i64) -> String {
    json!({
        "graph": graph,
        "fluent": "urn:fluent:scoped-count",
        "event": event,
        "subject": "urn:subject",
        "predicate": "urn:count",
        "property_shape": "urn:count-shape",
        "as_of": AT,
        "value": {"Integer": value},
    })
    .to_string()
}

#[tokio::test]
async fn scoped_writer_appends_fluent_on_its_graph_and_is_refused_elsewhere() {
    let mut store = Store::open_in_memory().unwrap();
    store
        .load_shapes(
            "quipu-fluents-v1",
            include_str!("../../shapes/fluents-v1.ttl"),
            AT,
        )
        .unwrap();
    store
        .load_shapes(
            "scoped-binding",
            &format!(
                "<urn:count-shape> <http://www.w3.org/ns/shacl#path> <urn:count>; \
                 <http://www.w3.org/ns/shacl#targetNode> <urn:subject>; \
                 <{}currentValuePolicy> \"max\" .",
                quipu::namespace::QUIPU_FLUENT
            ),
            AT,
        )
        .unwrap();
    for graph in ["urn:g:a", "urn:g:b"] {
        store.graph_create(graph).unwrap();
    }
    // alice may append fluents to urn:g:a only; bob holds an ordinary
    // update grant on urn:g:b but no fluent operation.
    let alice = grant(&store, "alice", "urn:g:a", &[WriteOperation::FluentAppend]);
    store.writer_grant_apply(&alice).unwrap();
    let bob = grant(&store, "bob", "urn:g:b", &[WriteOperation::Update]);
    store.writer_grant_apply(&bob).unwrap();
    let shared = Arc::new(crate::StoreHandle::writer_only(store));
    let policy = BearerPolicy::new(Some("fixture-operator".into()), None, None, 0)
        .unwrap()
        .with_scoped_writers(registry())
        .unwrap();
    let router = Router::new()
        .route("/fluent/observations", post(crate::fluents::append))
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
        let path = "/fluent/observations";
        let mime = "application/json";
        // Admitted on the granted graph, attributed to the scoped principal.
        let body = observation("urn:g:a", "urn:event:a1", 3);
        let (status, report) = request(&address, "alice", path, &body, mime);
        assert_eq!(status, 200, "{report}");
        let tx = report["tx"].as_i64().unwrap();
        let evidence = shared.lock().transaction_auth(tx).unwrap().unwrap();
        assert_eq!(evidence.principal, "urn:writer:alice");
        assert_eq!(evidence.credential_id.as_deref(), Some("alice"));
        assert_eq!(evidence.auth_class, "scoped_named_bearer");
        // A duplicate delivery replays the original transaction.
        let (status, replay) = request(&address, "alice", path, &body, mime);
        assert_eq!(status, 200, "{replay}");
        assert_eq!(replay["tx"].as_i64(), Some(tx));
        let snapshot = shared
            .lock()
            .fluent_snapshot("urn:g:a", "urn:fluent:scoped-count")
            .unwrap();
        assert_eq!(
            snapshot.producing_txs,
            BTreeSet::from([u64::try_from(tx).unwrap()])
        );

        // Refused on ROOT, on another graph, and without the fluent operation.
        let before = shared.lock().latest_tx_id().unwrap();
        for (who, graph, event) in [
            (
                "alice",
                quipu::schema::ROOT_GRAPH_IRI,
                "urn:never-root-event",
            ),
            ("alice", "urn:g:b", "urn:never-foreign-event"),
            ("bob", "urn:g:b", "urn:never-bob-event"),
        ] {
            let (status, out) = request(&address, who, path, &observation(graph, event, 9), mime);
            assert_eq!(status, 403, "{who} {graph}: {out}");
            assert_eq!(shared.lock().latest_tx_id().unwrap(), before);
            assert_eq!(shared.lock().lookup(event).unwrap(), None);
        }
        // A withdrawn grant refuses the next append without a restart.
        let mut revoke = grant(
            &shared.lock(),
            "alice",
            "urn:g:a",
            &[WriteOperation::FluentAppend],
        );
        revoke.revision = 2;
        revoke.revoked = true;
        shared.lock().writer_grant_apply(&revoke).unwrap();
        let (status, out) = request(
            &address,
            "alice",
            path,
            &observation("urn:g:a", "urn:event:a2", 4),
            mime,
        );
        assert_eq!(status, 403, "{out}");
        assert_eq!(shared.lock().latest_tx_id().unwrap(), before);
    })
    .await
    .unwrap();
    serving.abort();
}
