//! Offline demonstration: default advisory versus request-local strict admission.
use quipu::{
    Store,
    mcp::tools::{tool_episode, tool_graph_create, tool_shapes},
};
use serde_json::json;

fn main() -> quipu::Result<()> {
    for strict in [false, true] {
        let mut store = Store::open_in_memory()?;
        store.set_base_ns(quipu::namespace::DEFAULT_BASE_NS.to_owned());
        let shape = format!(
            "@prefix sh: <http://www.w3.org/ns/shacl#> . <urn:s> a sh:NodeShape ; sh:targetClass <{}Document> .",
            store.base_ns()
        );
        tool_shapes(
            &mut store,
            &json!({"action":"load","name":"documents","turtle":shape}),
        )?;
        tool_graph_create(&store, &json!({"graph":"urn:example:scratch"}))?;
        tool_episode(
            &mut store,
            &json!({"name":"seed","graph":"urn:example:scratch",
            "nodes":[{"name":"document-alpha","type":"Document"}],"edges":[]}),
        )?;
        let input = json!({"name":"candidate","graph":"urn:example:scratch",
            "strict_new_entities":strict,"nodes":[{"name":"document-alpha-variant","type":"Document"}],"edges":[]});
        let identity = quipu::transaction_auth::Identity {
            principal: "urn:example:writer".into(),
            credential_id: Some("offline-fixture".into()),
            auth_class: "named_bearer".into(),
            attestation: Default::default(),
        };
        let before = store.latest_tx_id()?;
        let result = quipu::transaction_auth::with_identity(Some(identity), || {
            tool_episode(&mut store, &input)
        });
        println!(
            "{}",
            json!({"strict_new_entities":strict,
            "result":if result.is_ok() {"ADMITTED"} else {"REFUSED"},
            "transaction_changed":store.latest_tx_id()? != before,
            "global_enabled":store.resolution_config().enabled,
            "global_strict":store.resolution_config().strict_mode})
        );
        assert_eq!(result.is_err(), strict);
        assert_eq!(store.latest_tx_id()? == before, strict);
    }
    Ok(())
}
