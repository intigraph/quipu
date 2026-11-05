//! aegis-sdfo: search works out of the box on a fresh store, and an existing
//! store's write path is untouched by the new default.
use crate::{SearchConfig, Store};
use serde_json::json;

fn fact(store: &mut Store, label: &str) {
    crate::rdf::ingest_rdf(
        store,
        format!("<urn:x:alpha> <http://www.w3.org/2000/01/rdf-schema#label> \"{label}\" .")
            .as_bytes(),
        oxrdfio::RdfFormat::NTriples,
        None,
        "2026-11-05T00:00:00Z",
        None,
        None,
    )
    .unwrap();
}

fn unset() -> SearchConfig {
    let config = SearchConfig::default();
    assert_eq!(
        config.keyword, None,
        "the shipped default leaves keyword unset"
    );
    config
}

#[test]
fn a_fresh_store_resolves_keyword_on_and_answers_a_plain_query() {
    let mut store = Store::open_in_memory().unwrap();
    assert!(store.adopt_search_config(&unset()).unwrap());
    store.initialize_lexical_index().unwrap();
    fact(&mut store, "Alpha widget calibration");

    // No mode and no embedding: the out-of-the-box request from the README.
    let out = crate::tool_search(&store, &json!({"query": "widget"})).unwrap();
    let text = out.to_string();
    assert!(text.contains("urn:x:alpha"), "fact found: {text}");
    assert_eq!(out["mode_fallback"]["mode"], "keyword", "{text}");
    assert_eq!(out["mode_fallback"]["requested"], "semantic");
    assert!(
        out["mode_fallback"]["fix"]
            .as_str()
            .unwrap()
            .contains("model_path")
    );
}

#[test]
fn an_explicit_semantic_request_without_a_model_refuses_with_the_fix() {
    let mut store = Store::open_in_memory().unwrap();
    store.adopt_search_config(&unset()).unwrap();
    store.initialize_lexical_index().unwrap();
    fact(&mut store, "Alpha widget calibration");
    let err = crate::tool_search(&store, &json!({"query": "widget", "mode": "semantic"}))
        .unwrap_err()
        .to_string();
    assert!(err.contains("semantic search is unavailable"), "{err}");
    assert!(
        err.contains("mode=keyword") && err.contains("model_path"),
        "{err}"
    );
}

#[test]
fn an_explicit_keyword_mode_carries_no_fallback_note() {
    let mut store = Store::open_in_memory().unwrap();
    store.adopt_search_config(&unset()).unwrap();
    store.initialize_lexical_index().unwrap();
    fact(&mut store, "Alpha widget calibration");
    let out = crate::tool_search(&store, &json!({"query": "widget", "mode": "keyword"})).unwrap();
    assert!(out.get("mode_fallback").is_none(), "{out}");
}

#[test]
fn a_populated_store_without_an_index_keeps_keyword_off_and_gains_no_triggers() {
    let mut store = Store::open_in_memory().unwrap();
    fact(&mut store, "existing production fact");
    assert!(!store.adopt_search_config(&unset()).unwrap());
    assert_eq!(store.search_config().keyword, Some(false));
    assert!(
        store.lexical_progress().unwrap().is_none(),
        "resolution must not create the FTS schema or its write triggers"
    );
    // With keyword off and no model, a plain query still refuses (old behaviour),
    // now naming the fix instead of an internal type.
    let err = crate::tool_search(&store, &json!({"query": "existing"}))
        .unwrap_err()
        .to_string();
    assert!(err.contains("semantic search is unavailable"), "{err}");
}

#[test]
fn a_populated_store_whose_index_exists_resolves_on() {
    let mut store = Store::open_in_memory().unwrap();
    store.initialize_lexical_index().unwrap();
    fact(&mut store, "indexed fact");
    assert!(store.keyword_default().unwrap());
    assert!(store.adopt_search_config(&unset()).unwrap());
}

#[test]
fn an_explicit_setting_always_wins_over_the_default() {
    let mut store = Store::open_in_memory().unwrap();
    let mut off = SearchConfig::default();
    off.keyword = Some(false);
    assert!(
        !store.adopt_search_config(&off).unwrap(),
        "explicit false on a fresh store stays off"
    );
    fact(&mut store, "populated");
    let mut on = SearchConfig::default();
    on.keyword = Some(true);
    assert!(
        store.adopt_search_config(&on).unwrap(),
        "explicit true on a populated store stays on"
    );
}

#[test]
fn an_unresolved_keyword_reads_as_off() {
    // A store that never went through adoption must not gain FTS behaviour.
    let store = Store::open_in_memory().unwrap();
    assert_eq!(store.search_config().keyword, None);
    assert!(!store.search_config().keyword_enabled());
}

#[test]
fn search_capabilities_name_the_working_modes_and_the_fixes() {
    let mut store = Store::open_in_memory().unwrap();
    store.adopt_search_config(&unset()).unwrap();
    store.initialize_lexical_index().unwrap();
    let report = store.search_capabilities().unwrap();
    assert_eq!(report["keyword"]["works"], true, "{report}");
    assert_eq!(report["semantic"]["works"], false, "{report}");
    let fix = report["semantic"]["fix"].as_str().unwrap();
    if cfg!(feature = "onnx") {
        assert!(fix.contains("model_path"), "{report}");
    } else {
        assert!(fix.contains("--features full"), "{report}");
    }

    let mut populated = Store::open_in_memory().unwrap();
    fact(&mut populated, "existing");
    populated.adopt_search_config(&unset()).unwrap();
    let report = populated.search_capabilities().unwrap();
    assert_eq!(report["keyword"]["works"], false, "{report}");
    assert!(
        report["keyword"]["fix"]
            .as_str()
            .unwrap()
            .contains("backfill")
    );
}
