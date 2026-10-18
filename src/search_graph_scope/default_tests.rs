use super::{tests::fixture, *};
use serde_json::json;

#[test]
fn omitted_keyword_scope_covers_root_and_named_graphs_with_visible_coverage() {
    let s = fixture();
    let out = crate::tool_search(&s, &json!({"mode":"keyword","query":"shared"})).unwrap();
    assert_eq!(out["count"], 3);
    assert_eq!(out["search_scope"]["selection"], "default");
    assert_eq!(out["search_scope"]["default"], "all");
    assert_eq!(out["search_scope"]["graphs"].as_array().unwrap().len(), 3);
    assert_eq!(out["search_scope"]["index_completeness"], "not_certified");
    assert!(
        out["results"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["graph"].is_string())
    );
}

#[test]
fn omitted_semantic_scope_finds_named_only_entity_and_exclusion_precedes_limit() {
    let mut s = fixture();
    for (iri, v) in [
        ("urn:root", vec![0.0, 1.0]),
        ("urn:alpha", vec![1.0, 0.0]),
        ("urn:beta", vec![0.8, 0.2]),
    ] {
        s.vector_store()
            .embed_entity(s.lookup(iri).unwrap().unwrap(), iri, &v, tests::AT)
            .unwrap();
    }
    let input = json!({"embedding":[1.0,0.0],"verbose":true,"limit":1});
    assert_eq!(
        crate::tool_search(&s, &input).unwrap()["results"][0]["entity"],
        "urn:alpha"
    );
    s.search_config_mut().exclude_graphs = vec!["urn:graph:a".into()];
    let out = crate::tool_search(&s, &input).unwrap();
    assert_eq!(out["results"][0]["entity"], "urn:beta");
    assert_eq!(
        out["search_scope"]["excluded_graphs"],
        json!(["urn:graph:a"])
    );
    assert!(crate::tool_search(&s, &json!({"embedding":[1.0,0.0],"graph":"urn:graph:a"})).is_err());
}

#[test]
fn legacy_root_default_is_configurable_and_bad_configuration_refuses() {
    let mut s = fixture();
    s.search_config_mut().default_graph_scope = "root".into();
    let input = json!({"mode":"keyword","query":"shared"});
    let legacy = crate::tool_search(&s, &input).unwrap();
    assert_eq!(legacy["count"], 1);
    println!("same input: {input}");
    println!(
        "ROOT compatibility: count={}, scope={}",
        legacy["count"], legacy["search_scope"]
    );
    s.search_config_mut().default_graph_scope = "all".into();
    let broad = crate::tool_search(&s, &input).unwrap();
    assert_eq!(broad["count"], 3);
    println!(
        "all-graph default: count={}, scope={}",
        broad["count"], broad["search_scope"]
    );
    assert_eq!(
        crate::tool_search(
            &s,
            &json!({"mode":"keyword","query":"shared","all_graphs":true})
        )
        .unwrap()["count"],
        3
    );
    s.search_config_mut().default_graph_scope = "typo".into();
    assert!(crate::tool_search(&s, &input).is_err());
    s.search_config_mut().default_graph_scope = "all".into();
    s.search_config_mut().exclude_graphs = vec!["not an iri".into()];
    assert!(crate::tool_search(&s, &input).is_err());
}

#[test]
fn exclusions_cover_keyword_and_root_and_deserialize_from_configuration() {
    let mut s = fixture();
    let config: crate::config::SearchConfig = toml::from_str(
        "default_graph_scope = 'all'\nexclude_graphs = ['urn:graph:a', 'urn:quipu:graph:root']\nkeyword = true",
    ).unwrap();
    *s.search_config_mut() = config;
    let out = crate::tool_search(
        &s,
        &json!({"mode":"keyword","query":"shared","verbose":true}),
    )
    .unwrap();
    assert_eq!(out["count"], 1);
    assert_eq!(out["results"][0]["entity"], "urn:beta");
    assert_eq!(out["search_scope"]["graphs"], json!(["urn:graph:b"]));
    assert!(
        crate::tool_search(
            &s,
            &json!({"mode":"keyword","query":"shared","graph":crate::schema::ROOT_GRAPH_IRI})
        )
        .is_err()
    );
}

#[test]
fn unprepared_default_named_semantic_search_refuses_instead_of_returning_root() {
    let mut s = fixture();
    s.search_config_mut().named_graphs = false;
    let err = crate::tool_search(&s, &json!({"embedding":[1.0,0.0]}))
        .unwrap_err()
        .to_string();
    assert!(err.contains("named_graphs = false"));
    assert!(err.contains("bounded backfill"));
    assert!(
        crate::tool_search(
            &s,
            &json!({"embedding":[1.0,0.0],"graph":crate::schema::ROOT_GRAPH_IRI})
        )
        .is_ok()
    );
    assert_eq!(
        crate::tool_search(&s, &json!({"mode":"keyword","query":"shared"})).unwrap()["count"],
        3
    );
}
