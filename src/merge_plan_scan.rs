//! Bounded before/after scans for the data merge planner.
#[cfg(feature = "shacl")]
use std::collections::BTreeSet;

use crate::error::{Error, Result};
use crate::share_merge::Graph;
use crate::sparql::QueryResult;
use crate::store::Store;

pub(super) fn scratch(graph: &Graph) -> Result<Store> {
    let mut store = Store::open_in_memory()?;
    crate::rdf::ingest_rdf_chunked(
        &mut store,
        crate::merge_session::render(graph).as_bytes(),
        oxrdfio::RdfFormat::NTriples,
        None,
        "2000-01-01T00:00:00Z",
        None,
        None,
        crate::schema::ROOT_GRAPH,
        10_000,
    )?;
    Ok(store)
}

fn answer(store: &Store, query: &str) -> Result<String> {
    let result = crate::sparql::query(store, query)?;
    let value = match result {
        QueryResult::Ask(value) => format!("ask:{value}"),
        QueryResult::Select {
            mut variables,
            rows,
        } => {
            if rows.len() > 10_000 {
                return Err(Error::InvalidValue(
                    "query result exceeds merge plan bound".into(),
                ));
            }
            variables.sort();
            let mut canonical = Vec::new();
            for row in rows {
                let bindings = variables
                    .iter()
                    .map(|variable| {
                        row.get(variable)
                            .map(|v| crate::rdf::value_to_term(store, v).map(|v| v.to_string()))
                            .transpose()
                    })
                    .collect::<Result<Vec<_>>>()?;
                canonical.push(bindings);
            }
            // SPARQL SELECT is a bag: keep repeated bindings, not a set.
            canonical.sort();
            serde_json::to_string(&(variables, canonical))
                .map_err(|e| Error::Serialization(e.to_string()))?
        }
        QueryResult::Graph(_) => {
            return Err(Error::InvalidValue(
                "only SELECT/ASK merge comparisons qualified".into(),
            ));
        }
    };
    Ok(crate::share::sha256(value.as_bytes()))
}

/// Parse before admitting the local evaluator. Unsupported algebra never runs,
/// even when keyword spelling, comments or escapes defeat a text filter.
fn supported(template: &str) -> bool {
    use spargebra::Query;
    use spargebra::algebra::{Expression, GraphPattern as GP};
    fn expression(expr: &Expression) -> bool {
        match expr {
            Expression::Exists(pattern) => pattern_ok(pattern),
            Expression::Not(inner) => expression(inner),
            _ => false,
        }
    }
    fn pattern_ok(pattern: &GP) -> bool {
        match pattern {
            GP::Bgp { patterns } => patterns.len() <= 4,
            GP::Join { left, right } | GP::Union { left, right } | GP::Minus { left, right } => {
                pattern_ok(left) && pattern_ok(right)
            }
            GP::LeftJoin {
                left,
                right,
                expression,
            } => expression.is_none() && pattern_ok(left) && pattern_ok(right),
            GP::Project { inner, .. } | GP::Distinct { inner } => pattern_ok(inner),
            GP::Filter { expr, inner } => expression(expr) && pattern_ok(inner),
            _ => false,
        }
    }
    let parsed = crate::sparql_structure::parse_query(crate::sparql::sparql_parser(), template)
        .ok()
        .and_then(std::result::Result::ok);
    match parsed {
        Some(
            Query::Select {
                dataset, pattern, ..
            }
            | Query::Ask {
                dataset, pattern, ..
            },
        ) => dataset.is_none() && pattern_ok(&pattern),
        _ => false,
    }
}

pub(super) fn queries(
    source: &Store,
    before: &Store,
    after: &Store,
    recall: Option<&std::collections::BTreeMap<String, String>>,
) -> Result<Vec<super::QueryChange>> {
    if !source.list_ontologies()?.is_empty() {
        return Err(Error::InvalidValue(
            "OWL query context needs qualification".into(),
        ));
    }
    let queries = source.query_list()?;
    if queries.len() > 1_000 {
        return Err(Error::InvalidValue(
            "query catalog exceeds merge plan bound".into(),
        ));
    }
    let mut changes = Vec::new();
    for query in queries {
        if !query.params.is_empty() || query.dataset.is_some() || !supported(&query.template) {
            return Err(Error::InvalidValue(format!(
                "query {} has unqualified bindings, scope or operators",
                query.name
            )));
        }
        let (before, after) = (
            answer(before, &query.template)?,
            answer(after, &query.template)?,
        );
        if before != after {
            changes.push(super::QueryChange {
                name: query.name,
                before,
                after,
            });
        }
    }
    for (name, query) in recall.into_iter().flatten() {
        if !supported(query) {
            return Err(Error::InvalidValue(format!(
                "recall probe {name} is not qualified"
            )));
        }
        let (before, after) = (answer(before, query)?, answer(after, query)?);
        if before != after {
            changes.push(super::QueryChange {
                name: format!("recall:{name}"),
                before,
                after,
            });
        }
    }
    changes.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(changes)
}

#[cfg(feature = "shacl")]
pub(super) fn issues(shapes: &str, before: &Graph, after: &Graph) -> Result<(usize, usize, usize)> {
    if shapes.trim().is_empty() {
        return Ok((0, 0, 0));
    }
    let terms = crate::proposal_plan::terms(shapes)?;
    if !terms.incomplete_reasons().is_empty() {
        return Err(Error::InvalidValue(terms.incomplete_reasons().join("; ")));
    }
    let validator = crate::shacl::Validator::from_turtle(shapes)?;
    let key = |issue: crate::shacl::ValidationIssue| crate::proposal_migration::issue_key(&issue);
    let before: BTreeSet<_> = validator
        .validate(crate::merge_session::render(before).as_bytes())?
        .results
        .into_iter()
        .map(key)
        .collect();
    let after: BTreeSet<_> = validator
        .validate(crate::merge_session::render(after).as_bytes())?
        .results
        .into_iter()
        .map(key)
        .collect();
    let newly: Vec<_> = after.difference(&before).collect();
    let instances: BTreeSet<_> = newly.iter().map(|issue| &issue.0).collect();
    Ok((before.len(), newly.len(), instances.len()))
}

#[cfg(not(feature = "shacl"))]
pub(super) fn issues(shapes: &str, _: &Graph, _: &Graph) -> Result<(usize, usize, usize)> {
    if shapes.trim().is_empty() {
        Ok((0, 0, 0))
    } else {
        Err(Error::InvalidValue("SHACL is compiled out".into()))
    }
}
