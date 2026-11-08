//! Retained parent ids never substitute for authenticated remainder evidence.
use super::*;
use crate::merge_ingress::{File, SCHEMA};
use crate::transaction_auth::{Identity, with_identity};
use serde_json::{Value as Json, json};

const LEDGER: &str = "urn:fixture:parent-ledger";
fn identity(name: &str) -> Identity {
    Identity {
        principal: format!("urn:fixture:{name}"),
        credential_id: Some(name.into()),
        auth_class: "named_bearer".into(),
        attestation: Default::default(),
    }
}
fn retained(request: Request, authored_at: &str, choices: Vec<Choice>) -> Json {
    serde_json::to_value(File {
        schema: SCHEMA.into(),
        request,
        authored_at: authored_at.into(),
        choices,
    })
    .unwrap()
}
fn apply(store: &mut Store, decisions: &Json) -> crate::Result<Json> {
    crate::mcp::merge_decisions::tool_merge_apply(
        store,
        &json!({
            "decisions":decisions, "ledger":LEDGER, "reviewer":"fixture"
        }),
    )
}
fn no_write(store: &mut Store, file: &Json) -> String {
    let tx = store.latest_tx_id().unwrap();
    let root = crate::share_merge::root_graph(store).unwrap().0;
    let id = crate::merge_ingress::id(&crate::merge_ingress::decode(file).unwrap()).unwrap();
    let error = apply(store, file)
        .expect_err("unverified parent must refuse")
        .to_string();
    assert_eq!(
        store.latest_tx_id().unwrap(),
        tx,
        "no receive/decide/receipt tx"
    );
    assert_eq!(crate::share_merge::root_graph(store).unwrap().0, root);
    assert!(
        crate::merge_pending::load(store, LEDGER, &id)
            .unwrap()
            .is_none()
    );
    error
}

#[cfg(feature = "shacl")]
fn pending(store: &mut Store, request: Request) -> Json {
    let plan = crate::merge_adapters::prepare_store(
        store,
        request,
        &[Choice {
            slot: key("p"),
            decision: Decision::Theirs,
        }],
        TT,
        TD,
    )
    .unwrap();
    let receipt = crate::merge_pending::execute(store, LEDGER, plan, TT, TD, None).unwrap();
    assert_eq!(receipt.held, 1);
    let choices = vec![Choice {
        slot: key("q"),
        decision: Decision::Theirs,
    }];
    let child = crate::merge_adapters::resume(store, LEDGER, &receipt.id, &choices, TD).unwrap();
    retained(child.request, TT, choices)
}

#[cfg(feature = "shacl")]
#[test]
fn nonexistent_parent_cannot_skip_inline_share_verification() {
    with_identity(Some(identity("alice")), || {
        let (mut store, mut request) = setup();
        request.parent = Some("nonexistent-parent".into());
        let file = retained(
            request,
            TT,
            vec![
                Choice {
                    slot: key("p"),
                    decision: Decision::Theirs,
                },
                Choice {
                    slot: key("q"),
                    decision: Decision::Theirs,
                },
            ],
        );
        assert!(no_write(&mut store, &file).contains("unknown parent"));
    });
}

#[cfg(feature = "shacl")]
#[test]
fn foreign_owner_parent_cannot_authorize_new_child() {
    let (mut store, request) = setup();
    let file = with_identity(Some(identity("alice")), || pending(&mut store, request));
    with_identity(Some(identity("bob")), || {
        assert!(no_write(&mut store, &file).contains("another authenticated writer"));
    });
}

#[cfg(feature = "shacl")]
#[test]
fn resumed_inputs_selection_and_authored_time_are_bound_before_writes() {
    for field in [
        "input_id",
        "base_id",
        "base",
        "theirs",
        "shapes",
        "profile",
        "active_slots",
        "store_id",
        "root_tx",
        "ours",
        "authored_at",
    ] {
        with_identity(Some(identity("alice")), || {
            let (mut store, request) = setup();
            let mut file = pending(&mut store, request);
            match field {
                "input_id" | "base_id" | "store_id" => file["request"][field] = json!("forged"),
                "root_tx" => file["request"][field] = json!(0),
                "ours" => file["request"][field] = json!(""),
                "base" => file["request"][field] = json!("<urn:e> <urn:q> \"forged-base\" .\n"),
                "theirs" => {
                    file["request"][field] = json!("<urn:e> <urn:q> \"forged-incoming\" .\n");
                }
                "shapes" => file["request"][field] = json!(""),
                "profile" => file["request"][field]["mode"] = json!("atomic"),
                "active_slots" => file["request"][field] = Json::Null,
                "authored_at" => file[field] = json!("2026-01-04T00:00:00Z"),
                _ => unreachable!(),
            }
            assert!(!no_write(&mut store, &file).is_empty(), "{field}");
        });
    }
}

#[cfg(feature = "shacl")]
#[test]
fn legitimate_parent_remainder_applies_and_replay_writes_nothing() {
    with_identity(Some(identity("alice")), || {
        let (mut store, request) = setup();
        let file = pending(&mut store, request);
        let before = store.latest_tx_id().unwrap();
        let result = apply(&mut store, &file).unwrap();
        assert_eq!(result["outcome"], "applied");
        assert_eq!(result["receipt"]["held"], 0);
        assert_eq!(result["receipt"]["applied"], 1);
        assert!(store.latest_tx_id().unwrap() > before);
        let tx = store.latest_tx_id().unwrap();
        let root = crate::share_merge::root_graph(&store).unwrap().0;
        let replay = apply(&mut store, &file).unwrap();
        assert_eq!(replay["outcome"], "already-applied");
        assert_eq!(replay["receipt"]["id"], result["receipt"]["id"]);
        assert_eq!(store.latest_tx_id().unwrap(), tx);
        assert_eq!(crate::share_merge::root_graph(&store).unwrap().0, root);
    });
}
