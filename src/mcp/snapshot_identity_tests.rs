use super::*;

const BEFORE: &str = "2026-09-01T00:00:00Z";
const AFTER: &str = "2026-09-02T00:00:00Z";

fn publish(store: &mut Store, key: &str, body: &str, timestamp: &str) {
    let result = tool_knot(store, &serde_json::json!({
        "snapshot": key, "replace_snapshot": true,
        "turtle": format!("@prefix ex: <http://example.org/> . @prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> . {body}"),
        "timestamp": timestamp,
    })).unwrap();
    assert_eq!(result["conforms"], true);
}

fn facts(store: &Store, name: &str) -> Vec<crate::types::Fact> {
    store
        .entity_facts(
            store
                .lookup(&format!("http://example.org/{name}"))
                .unwrap()
                .unwrap(),
        )
        .unwrap()
}

fn fixture() -> Store {
    let mut store = Store::open_in_memory().unwrap();
    for name in ["alpha", "beta", "gamma"] {
        publish(
            &mut store,
            &format!("code:tiny:src/{name}.rs"),
            &format!("ex:{name} a ex:Module ; rdfs:label \"{name}.rs\" ."),
            BEFORE,
        );
    }
    store
}

#[test]
fn sibling_code_partitions_do_not_preserve_deleted_identity() {
    let mut store = fixture();
    publish(
        &mut store,
        "code:tiny:src/lib.rs",
        "ex:lib ex:imports ex:beta .",
        BEFORE,
    );
    publish(
        &mut store,
        "code:tiny::provenance",
        "ex:commit ex:modifies ex:beta .",
        BEFORE,
    );
    // A sibling can mention the deleted entity as either subject or object.
    publish(
        &mut store,
        "code:tiny:src/extra.rs",
        "ex:beta ex:referencedBy ex:extra .",
        BEFORE,
    );
    publish(&mut store, "code:tiny:src/beta.rs", "", AFTER);
    let remaining = facts(&store, "beta");
    assert_eq!(
        remaining.len(),
        1,
        "only the sibling-owned fact should remain"
    );
    assert_eq!(
        store.resolve(remaining[0].attribute).unwrap(),
        "http://example.org/referencedBy"
    );
    // Same timestamp, distinct transactions; no ordering can resolve import cycles.
    publish(
        &mut store,
        "code:tiny:src/lib.rs",
        "ex:lib ex:imports ex:alpha .",
        AFTER,
    );
    publish(
        &mut store,
        "code:tiny::provenance",
        "ex:next ex:modifies ex:alpha .",
        AFTER,
    );
    publish(&mut store, "code:tiny:src/extra.rs", "", AFTER);
    assert!(
        facts(&store, "beta").is_empty(),
        "deleted file must leave no dangling label"
    );
    for name in ["alpha", "gamma"] {
        assert_eq!(
            facts(&store, name).len(),
            2,
            "untouched file identity must survive"
        );
    }
}

#[test]
fn foreign_producers_preserve_labels_but_not_stale_types() {
    for source in [
        Some("snapshot:code:other:src/lib.rs"),
        Some("snapshot:code:tiny2:src/lib.rs"),
        Some("snapshot:bobbin-chunks:tiny"),
        Some("owl:materialize"),
        Some("episode:external"),
        None,
    ] {
        for body in [
            "ex:foreign ex:imports ex:beta .",
            "ex:beta ex:referencedBy ex:foreign .",
        ] {
            let mut store = fixture();
            let result = tool_knot(
                &mut store,
                &serde_json::json!({
                    "turtle": format!("@prefix ex: <http://example.org/> . {body}"),
                    "source": source,
                    "timestamp": BEFORE,
                }),
            )
            .unwrap();
            assert_eq!(result["conforms"], true);
            publish(&mut store, "code:tiny:src/beta.rs", "", AFTER);
            let remaining = facts(&store, "beta");
            let predicates: Vec<_> = remaining
                .iter()
                .map(|f| store.resolve(f.attribute).unwrap())
                .collect();
            assert!(
                predicates
                    .iter()
                    .any(|p| p == "http://www.w3.org/2000/01/rdf-schema#label"),
                "foreign source {source:?} must preserve a label"
            );
            assert!(
                !predicates
                    .iter()
                    .any(|p| p == "http://www.w3.org/1999/02/22-rdf-syntax-ns#type")
            );
        }
    }
}

#[test]
fn producer_boundaries_are_literal_and_only_apply_to_code_snapshots() {
    for (owner, referrer, keep_label) in [
        ("code:tiny:src/beta.rs", "code:tiny", false),
        ("code:tiny", "code:tiny:src/lib.rs", false),
        ("code:tiny_%:src/beta.rs", "code:tiny_X:src/lib.rs", true),
        ("code:tiny_%:src/beta.rs", "code:tiny_%:src/lib.rs", false),
        ("other:tiny:src/beta.rs", "other:tiny:src/lib.rs", true),
        ("code::src/beta.rs", "code::src/lib.rs", true),
    ] {
        let mut store = Store::open_in_memory().unwrap();
        publish(
            &mut store,
            owner,
            "ex:beta a ex:Module ; rdfs:label \"beta.rs\" .",
            BEFORE,
        );
        publish(
            &mut store,
            referrer,
            "ex:foreign ex:imports ex:beta .",
            BEFORE,
        );
        publish(&mut store, owner, "", AFTER);
        assert_eq!(
            facts(&store, "beta").len(),
            usize::from(keep_label),
            "owner={owner}, referrer={referrer}"
        );
    }
}

#[test]
fn replacing_one_partition_leaves_untouched_file_facts_unchanged() {
    let mut store = fixture();
    let before = format!("{:?}{:?}", facts(&store, "alpha"), facts(&store, "gamma"));
    publish(
        &mut store,
        "code:tiny::provenance",
        "ex:commit ex:modifies ex:beta .",
        BEFORE,
    );
    publish(&mut store, "code:tiny:src/beta.rs", "", AFTER);
    assert_eq!(
        format!("{:?}{:?}", facts(&store, "alpha"), facts(&store, "gamma")),
        before
    );
}

fn labels(store: &Store, name: &str) -> Vec<String> {
    let mut out: Vec<String> = facts(store, name)
        .into_iter()
        .filter(|f| {
            store.resolve(f.attribute).unwrap() == "http://www.w3.org/2000/01/rdf-schema#label"
        })
        .map(|f| match f.value {
            crate::types::Value::Str(s) => s,
            other => format!("{other:?}"),
        })
        .collect();
    out.sort();
    out
}

/// aegis-p9q: the owner RENAMES a node another producer still references. The
/// replacement names the node itself, so the old label has no orphan to
/// protect and must go: exactly the new label remains.
#[test]
fn owner_rename_of_foreign_referenced_node_keeps_only_the_new_label() {
    let mut store = Store::open_in_memory().unwrap();
    publish(
        &mut store,
        "docs:owner",
        "ex:section a ex:Section ; rdfs:label \"Old name\" .",
        BEFORE,
    );
    publish(
        &mut store,
        "docs:other",
        "ex:foreign ex:cites ex:section .",
        BEFORE,
    );
    publish(
        &mut store,
        "docs:owner",
        "ex:section a ex:Section ; rdfs:label \"New name\" .",
        AFTER,
    );
    assert_eq!(labels(&store, "section"), vec!["New name".to_string()]);
    // The foreign reference is untouched.
    let foreign = facts(&store, "foreign");
    assert_eq!(foreign.len(), 1, "foreign producer's fact must survive");
}

/// Existing intent, pinned beside the rename case: when the owner REMOVES a
/// foreign-referenced node, its old label is retained so the reference is not
/// left unnameable.
#[test]
fn owner_removal_of_foreign_referenced_node_retains_old_label() {
    let mut store = Store::open_in_memory().unwrap();
    publish(
        &mut store,
        "docs:owner",
        "ex:section a ex:Section ; rdfs:label \"Old name\" .",
        BEFORE,
    );
    publish(
        &mut store,
        "docs:other",
        "ex:foreign ex:cites ex:section .",
        BEFORE,
    );
    publish(&mut store, "docs:owner", "ex:other a ex:Section .", AFTER);
    assert_eq!(labels(&store, "section"), vec!["Old name".to_string()]);
    // A replacement that keeps the node but drops its label also leaves it
    // unnamed, so retention still applies.
    let mut store = Store::open_in_memory().unwrap();
    publish(
        &mut store,
        "docs:owner",
        "ex:section a ex:Section ; rdfs:label \"Old name\" .",
        BEFORE,
    );
    publish(
        &mut store,
        "docs:other",
        "ex:foreign ex:cites ex:section .",
        BEFORE,
    );
    publish(&mut store, "docs:owner", "ex:section a ex:Section .", AFTER);
    assert_eq!(labels(&store, "section"), vec!["Old name".to_string()]);
}

/// No foreign reference: a rename has always replaced the label outright.
#[test]
fn owner_rename_without_foreign_reference_retracts_old_label() {
    let mut store = Store::open_in_memory().unwrap();
    publish(
        &mut store,
        "docs:owner",
        "ex:section a ex:Section ; rdfs:label \"Old name\" .",
        BEFORE,
    );
    publish(
        &mut store,
        "docs:owner",
        "ex:section a ex:Section ; rdfs:label \"New name\" .",
        AFTER,
    );
    assert_eq!(labels(&store, "section"), vec!["New name".to_string()]);
}

/// A label another producer asserted on the same node is not the snapshot's
/// to retract, rename or not.
#[test]
fn owner_rename_leaves_foreign_owned_label_alone() {
    let mut store = Store::open_in_memory().unwrap();
    publish(
        &mut store,
        "docs:owner",
        "ex:section a ex:Section ; rdfs:label \"Old name\" .",
        BEFORE,
    );
    publish(
        &mut store,
        "docs:other",
        "ex:section rdfs:label \"Alias\" . ex:foreign ex:cites ex:section .",
        BEFORE,
    );
    publish(
        &mut store,
        "docs:owner",
        "ex:section a ex:Section ; rdfs:label \"New name\" .",
        AFTER,
    );
    assert_eq!(
        labels(&store, "section"),
        vec!["Alias".to_string(), "New name".to_string()]
    );
}
