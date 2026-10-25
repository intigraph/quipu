//! Explicit application datasets for referenced SHACL types, never values.

use super::Store;
use crate::{Error, Result};

impl Store {
    /// Select the exact graphs that may supply referenced nodes' `rdf:type`
    /// during writes to `destination` made by `operation`.
    ///
    /// This library-only scope does not grant write authority, enable SHACL,
    /// merge cardinality values, or discover sibling graphs. ROOT is included
    /// only if explicitly selected. Writes to other destinations retain their
    /// default context. Nested scopes and unwinding restore the previous scope.
    pub fn with_write_reference_graphs<T>(
        &mut self,
        destination: i64,
        graphs: &[i64],
        operation: impl FnOnce(&mut Self) -> Result<T>,
    ) -> Result<T> {
        if graphs.is_empty() || graphs.len() > 16 || !graphs.contains(&destination) {
            return Err(Error::InvalidValue(
                "reference-type dataset requires its destination and at most 16 graphs".into(),
            ));
        }
        let previous = self
            .write_reference_graphs
            .replace((destination, graphs.to_vec()));
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| operation(self)));
        self.write_reference_graphs = previous;
        match result {
            Ok(result) => result,
            Err(panic) => std::panic::resume_unwind(panic),
        }
    }

    #[cfg(feature = "shacl")]
    pub(crate) fn reference_type_graphs(&self, destination: i64) -> Vec<i64> {
        let graphs = match &self.write_reference_graphs {
            Some((graph, graphs)) if *graph == destination => graphs.clone(),
            _ => vec![crate::schema::ROOT_GRAPH, destination],
        };
        graphs
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect()
    }

    #[cfg(feature = "shacl")]
    pub(crate) fn write_property_graphs(&self, destination: i64) -> Vec<i64> {
        match &self.write_reference_graphs {
            Some((graph, _)) if *graph == destination => vec![destination],
            _ => self.reference_type_graphs(destination),
        }
    }
}

#[cfg(all(test, feature = "shacl"))]
mod tests {
    use super::*;

    const SHAPES: &str = "@prefix sh: <http://www.w3.org/ns/shacl#> .
@prefix ex: <http://example.org/> .
ex:S a sh:NodeShape ; sh:targetClass ex:Action ;
 sh:property [ sh:path ex:blockedOn ; sh:class ex:Action ; sh:maxCount 1 ] .";

    fn write(store: &mut Store, graph: &str, data: &str) -> Result<()> {
        crate::tool_knot(
            store,
            &serde_json::json!({
                "graph": graph, "turtle": format!("@prefix ex: <http://example.org/> . {data}"),
                "timestamp": "2026-10-25T00:00:00Z"
            }),
        )
        .and_then(|result| {
            if result.get("conforms").and_then(serde_json::Value::as_bool) == Some(false) {
                Err(Error::InvalidValue(format!("SHACL refusal: {result}")))
            } else {
                Ok(())
            }
        })
    }

    #[test]
    fn explicit_reference_types_preserve_graph_isolation_on_both_paths() {
        for incremental in [false, true] {
            let mut store = Store::open_in_memory().unwrap();
            store.shacl_config.validate_on_write = true;
            store
                .load_shapes("refs", SHAPES, "2026-10-25T00:00:00Z")
                .unwrap();
            let project = store.graph_create("urn:project").unwrap();
            let ephemeral = store.graph_create("urn:project:eph").unwrap();
            store.graph_create("urn:other").unwrap();
            write(&mut store, "urn:project", "ex:target a ex:Action .").unwrap();
            write(&mut store, "urn:project", "ex:target2 a ex:Action .").unwrap();
            write(&mut store, "urn:other", "ex:foreign a ex:Action .").unwrap();
            write(&mut store, "urn:project", "ex:pseudo ex:name \"Action\" .").unwrap();
            crate::tool_knot(
                &mut store,
                &serde_json::json!({
                    "turtle": "<http://example.org/rootonly> a <http://example.org/Action> .",
                    "timestamp": "2026-10-25T00:00:00Z"
                }),
            )
            .unwrap();
            if incremental {
                write(
                    &mut store,
                    "urn:project:eph",
                    "ex:existing ex:name \"existing\" .",
                )
                .unwrap();
            }
            let allowed = "ex:source a ex:Action ; ex:blockedOn ex:target .";
            let unscoped = write(&mut store, "urn:project:eph", allowed);
            println!(
                "existing_graph={incremental}; destination=eph; default context; accepted={}",
                unscoped.is_ok()
            );
            assert!(unscoped.is_err());
            store
                .with_write_reference_graphs(ephemeral, &[ephemeral, project], |store| {
                let scoped = write(store, "urn:project:eph", allowed);
                println!("existing_graph={incremental}; destination=eph; context=[eph,project]; accepted={}", scoped.is_ok());
                scoped?;
                    assert!(
                        write(
                            store,
                            "urn:project:eph",
                            "ex:source ex:blockedOn ex:target2 ."
                        )
                        .is_err(),
                        "post-state cardinality still refuses a second value"
                    );
                    let source = store.lookup("http://example.org/source")?.unwrap();
                    let blocked = store.lookup("http://example.org/blockedOn")?.unwrap();
                    assert_eq!(
                        store
                            .entity_facts_in_graph(source, ephemeral)?
                            .iter()
                            .filter(|fact| fact.attribute == blocked)
                            .count(),
                        1,
                        "refused write rolled back"
                    );
                    write(store, "urn:project:eph", "ex:ephemeralTarget a ex:Action .")?;
                    assert!(
                        write(
                            store,
                            "urn:project",
                            "ex:reverse a ex:Action ; ex:blockedOn ex:ephemeralTarget ."
                        )
                        .is_err()
                    );
                    for target in ["foreign", "pseudo", "missing", "rootonly"] {
                        assert!(
                            write(
                                store,
                                "urn:project:eph",
                                &format!("ex:bad{target} a ex:Action ; ex:blockedOn ex:{target} .")
                            )
                            .is_err(),
                            "{target} must not satisfy sh:class"
                        );
                    }
                    // Type context must not merge property values from the project.
                    write(store, "urn:project", "ex:source ex:blockedOn ex:foreign .")?;
                    write(store, "urn:project:eph", allowed)?;
                    Ok(())
                })
                .unwrap();
            assert!(
                write(
                    &mut store,
                    "urn:project:eph",
                    "ex:after a ex:Action ; ex:blockedOn ex:target ."
                )
                .is_err()
            );
        }
    }

    #[test]
    fn referenced_types_do_not_create_additional_validation_focuses() {
        for incremental in [false, true] {
            let mut store = Store::open_in_memory().unwrap();
            let project = store.graph_create("urn:project").unwrap();
            let ephemeral = store.graph_create("urn:project:eph").unwrap();
            write(&mut store, "urn:project", "ex:target a ex:Target .").unwrap();
            if incremental {
                write(
                    &mut store,
                    "urn:project:eph",
                    "ex:existing ex:name \"existing\" .",
                )
                .unwrap();
            }
            let shapes = "@prefix sh: <http://www.w3.org/ns/shacl#> .
@prefix ex: <http://example.org/> .
@prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .
ex:S a sh:NodeShape ; sh:targetClass ex:Action ;
 sh:property [ sh:path ex:blockedOn ; sh:class ex:Target ] .
ex:T a sh:NodeShape ; sh:targetClass ex:Target ;
 sh:property [ sh:path rdf:type ; sh:nodeKind sh:Literal ] .";
            store
                .load_shapes("refs", shapes, "2026-10-25T00:00:00Z")
                .unwrap();
            store.shacl_config.validate_on_write = true;
            store
                .with_write_reference_graphs(ephemeral, &[ephemeral, project], |store| {
                    write(
                        store,
                        "urn:project:eph",
                        "ex:source a ex:Action ; ex:blockedOn ex:target .",
                    )
                })
                .expect("the referenced target's own debt is outside the write's focuses");
        }
    }

    #[test]
    fn reference_scope_restores_after_nested_error_and_panic() {
        let mut store = Store::open_in_memory().unwrap();
        let a = store.graph_create("urn:a").unwrap();
        let b = store.graph_create("urn:b").unwrap();
        store
            .with_write_reference_graphs(a, &[a, b], |store| {
                assert_eq!(store.reference_type_graphs(a), vec![a, b]);
                let nested: Result<()> = store.with_write_reference_graphs(a, &[a], |_| {
                    Err(Error::InvalidValue("expected".into()))
                });
                assert!(nested.is_err());
                assert_eq!(store.reference_type_graphs(a), vec![a, b]);
                let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let _: Result<()> =
                        store.with_write_reference_graphs(a, &[a], |_| panic!("expected"));
                }));
                assert!(panic.is_err());
                assert_eq!(store.reference_type_graphs(a), vec![a, b]);
                Ok(())
            })
            .unwrap();
        assert_eq!(store.reference_type_graphs(a), vec![0, a]);
    }
}
