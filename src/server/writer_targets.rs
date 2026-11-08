//! Mutation targets, BEFORE evaluation/interning, including empty WHERE arms.
use std::collections::BTreeSet;

use spargebra::{
    GraphUpdateOperation, SparqlParser,
    term::{GraphName, GraphNamePattern},
};

fn denied() -> quipu::Error {
    quipu::Error::PolicyDenied("scoped writer target cannot be authorized".into())
}

pub(crate) fn json(input: &serde_json::Value) -> BTreeSet<String> {
    // Omitted/empty/ill-typed graph remains ROOT (ungrantable), never inferred
    // from the credential's grant. The handler retains its own input checking.
    BTreeSet::from([input
        .get("graph")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .to_owned()])
}

pub(super) fn update(update: &str) -> quipu::Result<BTreeSet<String>> {
    let parsed = quipu::sparql_structure::parse_update(SparqlParser::new(), update)
        .map_err(|_| denied())?
        .map_err(|_| denied())?;
    let mut graphs = BTreeSet::new();
    for op in parsed.operations {
        match op {
            GraphUpdateOperation::InsertData { data } => {
                for q in data {
                    let GraphName::NamedNode(g) = q.graph_name else {
                        return Err(denied());
                    };
                    graphs.insert(g.as_str().into());
                }
            }
            GraphUpdateOperation::DeleteData { data } => {
                for q in data {
                    let GraphName::NamedNode(g) = q.graph_name else {
                        return Err(denied());
                    };
                    graphs.insert(g.as_str().into());
                }
            }
            GraphUpdateOperation::DeleteInsert { delete, insert, .. } => {
                for graph in delete
                    .into_iter()
                    .map(|q| q.graph_name)
                    .chain(insert.into_iter().map(|q| q.graph_name))
                {
                    let GraphNamePattern::NamedNode(g) = graph else {
                        return Err(denied());
                    };
                    graphs.insert(g.as_str().into());
                }
            }
            // LOAD/CLEAR/CREATE/DROP and their desugared aliases must not be
            // mistaken for ordinary conditional graph writes.
            _ => return Err(denied()),
        }
    }
    if graphs.is_empty() || graphs.len() > 64 {
        return Err(denied());
    }
    Ok(graphs)
}

/// The generic tool adapter shares the same all-or-nothing authorization.
pub(crate) fn tool(
    store: &mut quipu::Store,
    endpoint: &str,
    input: &serde_json::Value,
    call: impl FnOnce(&mut quipu::Store, &serde_json::Value) -> quipu::Result<serde_json::Value>,
) -> quipu::Result<serde_json::Value> {
    if !quipu::writer_scope::is_scoped_request() {
        return call(store, input);
    }
    let op = quipu::writer_scope::WriteOperation::for_route(endpoint).ok_or_else(denied)?;
    if op == quipu::writer_scope::WriteOperation::EntityRefMint
        && input.get("action").and_then(serde_json::Value::as_str) != Some("mint")
    {
        return Err(denied());
    }
    store.with_writer_scope(op, &json(input), |store| call(store, input))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn targets_are_static_and_not_observed_diff() {
        assert_eq!(
            update("INSERT { GRAPH <urn:g> { <urn:s> <urn:p> 1 } } WHERE { FILTER(false) }")
                .unwrap(),
            BTreeSet::from(["urn:g".into()])
        );
        assert_eq!(update("WITH <urn:g> DELETE { <urn:s> <urn:p> ?o } INSERT { <urn:s> <urn:p> 2 } WHERE { <urn:s> <urn:p> ?o }").unwrap(), BTreeSet::from(["urn:g".into()]));
        for text in [
            "INSERT DATA { <urn:s> <urn:p> 1 }",
            "INSERT { GRAPH ?g { <urn:s> <urn:p> 1 } } WHERE { VALUES ?g { <urn:g> } }",
            "CLEAR GRAPH <urn:g>",
            "CREATE GRAPH <urn:g>",
            "DROP SILENT GRAPH <urn:g>",
        ] {
            assert!(update(text).is_err(), "{text}");
        }
        assert_eq!(update("INSERT DATA { GRAPH <urn:a> { <urn:s> <urn:p> 1 } GRAPH <urn:b> { <urn:s> <urn:p> 2 } }").unwrap().len(), 2);
    }
}
