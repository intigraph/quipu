//! Pre-existing SHACL debt (aegis-g51c C12): which violations a write did not
//! introduce, and which a delete does.
//!
//! [`crate::shacl_context`] judges what a write asserts, with the store as
//! repair context. A node stored before its shape existed can still lack a
//! required value, and a writer that merely reuses it by name and type was
//! refused for that. Admission here demotes exactly that debt to a Warning;
//! the deletion check keeps a delete from creating such a node.

use std::collections::{BTreeMap, BTreeSet};

use oxrdf::{NamedOrBlankNode, Term as OxTerm};
use oxrdfio::{RdfFormat, RdfParser};

use crate::error::{Error, Result};
use crate::shacl_context::{
    RDF_TYPE, issue_key, repaired, required_paths, store_type_context_in_graph,
};
use crate::store::Store;
use crate::types::Value;

/// Prefix on the message of a violation admitted as pre-existing debt.
pub const DEBT_MESSAGE_PREFIX: &str = "pre-existing debt (admitted as Warning): ";

/// Demote to Warning each remaining violation the store ALREADY had (aegis-g51c C12).
///
/// # The defect
///
/// A writer reusing an existing node by name and type (`<s> a aegis:Session`,
/// the graph-extract reuse form) is judged by the stored node's required
/// fields, through the repair above. When the STORED node was written before
/// its shape existed and lacks a required field, the write is refused for a
/// violation it did not introduce and cannot fix without inventing provenance.
/// Measured in production: a session-capture `/episode` refused for an old
/// codex Session's missing `actor`/`sourceKind` (tx 722359), and 562 more such
/// nodes in ROOT.
///
/// # What is admitted, exactly
///
/// The PRE-STATE of each remaining focus node, meaning its whole stored
/// description plus the types of the nodes it references, is validated on its
/// own. Only focus-local components are admitted ([`is_focus_local`]): for those
/// that context is the complete pre-state, so a pre-state violation is real
/// debt. Nested and logical components (`sh:node`, `sh:qualifiedValueShape`,
/// `sh:or` and kin) depend on other nodes' descriptions; a bounded context could
/// fabricate their pre-state violation, so they are never admitted. A payload violation is admitted only
/// when that pre-state run reports the SAME issue: the same focus, component,
/// path, value, message and, when the shape is named, source shape. Everything
/// else is still refused:
///
/// - a NEW node has no stored types, so its pre-state targets no shape;
/// - a new TYPE on an existing node is not in the pre-state, so its shape's
///   violations have no pre-state twin;
/// - a new VALUE (`sh:class`, `sh:in`, datatype) differs in the key;
/// - a different path on a debt node differs in the key.
///
/// Admitted issues stay visible: each is returned as a Warning with
/// [`DEBT_MESSAGE_PREFIX`] and queued as a `shacl.debt_admitted` event, so the
/// debt can be counted and burnt down rather than becoming permanent.
#[cfg(feature = "shacl")]
pub(crate) fn admit_preexisting_debt(
    store: &Store,
    shapes_turtle: &str,
    feedback: crate::shacl::ValidationFeedback,
    payload: &str,
    augmented: Option<&str>,
    g: i64,
) -> Result<crate::shacl::ValidationFeedback> {
    let focuses: BTreeSet<String> = feedback
        .results
        .iter()
        .filter(|r| is_violation(r))
        .map(|r| r.focus_node.clone())
        .collect();
    // Only a focus the store already describes can carry debt, judged per focus:
    // a brand-new node (even one an `sh:targetNode` shape names) never can.
    let (pre_state, existing) = store_description_in_graph(store, &focuses, g)?;
    if existing.is_empty() {
        return Ok(feedback);
    }
    // ONE compiled validator for every run compared below. An anonymous shape
    // gets a fresh blank-node label per compile, and the shared cache can be
    // cleared between two calls, so the verdict being judged is re-derived from
    // the same instance as the pre-state: their source shapes are then
    // comparable, and two different anonymous shapes never share a key.
    let validator = crate::shacl::cached_validator(shapes_turtle)?;
    let baseline = validator.validate(payload.as_bytes())?;
    let mut feedback = match augmented {
        Some(doc) => repaired(baseline, &validator.validate(doc.as_bytes())?),
        None => baseline,
    };
    if feedback.conforms {
        return Ok(feedback);
    }
    let before = validator.validate(pre_state.as_bytes())?;
    let debt: BTreeSet<String> = before
        .results
        .iter()
        .filter(|r| is_violation(r) && is_focus_local(r) && existing.contains(&r.focus_node))
        .map(debt_key)
        .collect();
    if debt.is_empty() {
        return Ok(feedback);
    }

    let graph = store.graph_iri_of(g);
    for issue in &mut feedback.results {
        if !is_violation(issue)
            || !is_focus_local(issue)
            || !existing.contains(&issue.focus_node)
            || !debt.contains(&debt_key(issue))
        {
            continue;
        }
        store.queue_write_event(crate::store::PendingWriteEvent {
            event_type: "shacl.debt_admitted".to_string(),
            subject: Some(issue.focus_node.clone()),
            payload: serde_json::json!({
                "shape": issue.source_shape,
                "message": issue.message,
                "component": issue.component,
                "path": issue.path,
                "graph": graph,
            }),
        });
        issue.severity = "Warning".to_string();
        issue.message = Some(format!(
            "{DEBT_MESSAGE_PREFIX}{}",
            issue.message.as_deref().unwrap_or("")
        ));
    }
    feedback.violations = feedback.results.iter().filter(|r| is_violation(r)).count();
    feedback.warnings = feedback
        .results
        .iter()
        .filter(|r| r.severity.to_ascii_lowercase().contains("warning"))
        .count();
    feedback.conforms = feedback.violations == 0;
    Ok(feedback)
}

/// Violations a RETRACTION introduces on the nodes it edits (aegis-g51c C12).
///
/// Write-time SHACL judges what a write asserts, so `/update` could delete a
/// required field and leave a conforming node violating, unseen. Debt admission
/// makes that gap matter: a node made non-conforming by a delete would look like
/// pre-existing debt to every later write. So a delete is judged here against
/// the same pre-state [`admit_preexisting_debt`] uses.
///
/// `removed_nt` and `asserted_nt` are the N-Triples the update deletes from and
/// adds to graph `g`. Only subjects that lose a value on a path with a floor
/// (`sh:minCount >= 1` or `sh:hasValue`) are judged, on their full stored
/// description before and after; only those two components are reported,
/// since a delete can newly fail nothing else that is local to the node. A violation in that
/// post-state is returned unless the pre-state already had it, keyed by
/// [`debt_key`]. An empty result means the deletes introduce nothing.
#[cfg(feature = "shacl")]
pub fn retraction_violations_in_graph(
    store: &Store,
    shapes_turtle: &str,
    removed_nt: &str,
    asserted_nt: &str,
    g: i64,
) -> Result<crate::shacl::ValidationFeedback> {
    let sensitive = deletion_sensitive_paths(shapes_turtle)?;
    let removed = parse_triples(removed_nt)?;
    let subjects: BTreeSet<String> = removed
        .iter()
        .filter(|t| sensitive.contains(t.predicate.as_str()))
        .filter_map(|t| match &t.subject {
            NamedOrBlankNode::NamedNode(s) => Some(s.as_str().to_string()),
            NamedOrBlankNode::BlankNode(_) => None,
        })
        .collect();
    let mut feedback = crate::shacl::ValidationFeedback {
        conforms: true,
        violations: 0,
        warnings: 0,
        results: Vec::new(),
        resolution_candidates: Default::default(),
    };
    if subjects.is_empty() {
        return Ok(feedback);
    }

    // Full stored descriptions, so the floors judged below see every value.
    let (pre_nt, _) = store_description_in_graph(store, &subjects, g)?;
    // The post-state per graph: the deletes leave `g`, never ROOT, so a value
    // ROOT also holds survives them even when `g`'s copy is the one removed.
    let edited = describe(store, &subjects, &[g])?;
    let mut post: std::collections::HashSet<oxrdf::Triple> =
        parse_triples(&edited.lines.into_iter().collect::<String>())?
            .into_iter()
            .collect();
    for triple in &removed {
        post.remove(triple);
    }
    if g != crate::schema::ROOT_GRAPH {
        let kept = describe(store, &subjects, &[crate::schema::ROOT_GRAPH])?;
        post.extend(parse_triples(&kept.lines.into_iter().collect::<String>())?);
    }
    let mut referenced = edited.referenced;
    referenced.extend(describe(store, &subjects, &[crate::schema::ROOT_GRAPH])?.referenced);
    post.extend(parse_triples(&store_type_context_in_graph(
        store,
        &referenced,
        g,
    )?)?);
    post.extend(parse_triples(asserted_nt)?.into_iter().filter(
        |t| matches!(&t.subject, NamedOrBlankNode::NamedNode(s) if subjects.contains(s.as_str())),
    ));
    let post_nt: String = post.iter().map(|t| format!("{t} .\n")).collect();

    // One compiled validator for both states, as in [`admit_preexisting_debt`].
    let validator = crate::shacl::cached_validator(shapes_turtle)?;
    let before = validator.validate(pre_nt.as_bytes())?;
    let debt: BTreeSet<String> = before
        .results
        .iter()
        .filter(|r| is_violation(r))
        .map(debt_key)
        .collect();
    let after = validator.validate(post_nt.as_bytes())?;
    feedback.results = after
        .results
        .into_iter()
        // A delete takes a value away, so a floor is what it can newly fail:
        // a minimum count or a required value. Other components are not
        // judged on a delete.
        .filter(|r| is_violation(r) && subjects.contains(&r.focus_node))
        .filter(|r| {
            r.component.ends_with("MinCountConstraintComponent")
                || r.component.ends_with("HasValueConstraintComponent")
        })
        .filter(|r| !debt.contains(&debt_key(r)))
        .collect();
    feedback.violations = feedback.results.len();
    feedback.conforms = feedback.violations == 0;
    Ok(feedback)
}

#[cfg(feature = "shacl")]
fn parse_triples(nt: &str) -> Result<Vec<oxrdf::Triple>> {
    RdfParser::from_format(RdfFormat::NTriples)
        .for_reader(nt.as_bytes())
        .map(|quad| {
            quad.map(oxrdf::Triple::from)
                .map_err(|e| Error::InvalidValue(format!("RDF parse error: {e}")))
        })
        .collect()
}

/// Every current fact of `iris` in graph `g` and ROOT, plus the types of every
/// IRI those facts reference, as N-Triples: the pre-state debt is judged on.
/// Also returns which of `iris` have at least one stored fact.
#[cfg(feature = "shacl")]
fn store_description_in_graph(
    store: &Store,
    iris: &BTreeSet<String>,
    g: i64,
) -> Result<(String, BTreeSet<String>)> {
    let mut graphs = vec![crate::schema::ROOT_GRAPH];
    if g != crate::schema::ROOT_GRAPH {
        graphs.push(g);
    }
    let parts = describe(store, iris, &graphs)?;
    let mut out: String = parts.lines.into_iter().collect();
    out.push_str(&store_type_context_in_graph(store, &parts.referenced, g)?);
    Ok((out, parts.existing))
}

/// What [`describe`] read: N-Triples lines, the IRIs those facts reference,
/// and which of the described IRIs had any fact.
#[cfg(feature = "shacl")]
struct Described {
    lines: BTreeSet<String>,
    referenced: BTreeSet<String>,
    existing: BTreeSet<String>,
}

/// Every current fact of `iris` in the given graphs.
#[cfg(feature = "shacl")]
fn describe(store: &Store, iris: &BTreeSet<String>, graphs: &[i64]) -> Result<Described> {
    let mut parts = Described {
        lines: BTreeSet::new(),
        referenced: BTreeSet::new(),
        existing: BTreeSet::new(),
    };
    for iri in iris {
        let Some(entity) = store.lookup(iri)? else {
            continue;
        };
        for &scope in graphs {
            for fact in store.entity_facts_in_graph(entity, scope)? {
                let attr = store.resolve(fact.attribute)?;
                let Ok(term) = crate::rdf::value_to_term(store, &fact.value) else {
                    continue;
                };
                if let (Value::Ref(id), true) = (&fact.value, attr != RDF_TYPE) {
                    parts.referenced.insert(store.resolve(*id)?);
                }
                parts.lines.insert(format!("<{iri}> <{attr}> {term} .\n"));
                parts.existing.insert(iri.clone());
            }
        }
    }
    Ok(parts)
}

/// Components a node's own description and its references' types decide
/// completely, on a plain predicate path, AND whose key pins the offending
/// value (or, for a floor, its absence). Anything else is never debt.
#[cfg(feature = "shacl")]
fn is_focus_local(issue: &crate::shacl::ValidationIssue) -> bool {
    const LOCAL: &[&str] = &[
        // MaxCount is deliberately absent: its result carries no value, so a
        // write that ADDS values to an over-count path would match the old
        // debt key while making the debt worse.
        "MinCountConstraintComponent",
        "HasValueConstraintComponent",
        "InConstraintComponent",
        "DatatypeConstraintComponent",
        "NodeKindConstraintComponent",
        "PatternConstraintComponent",
        "MinLengthConstraintComponent",
        "MaxLengthConstraintComponent",
        "LanguageInConstraintComponent",
        "ClassConstraintComponent",
    ];
    let component = issue
        .component
        .trim_matches(['<', '>'])
        .trim_start_matches("http://www.w3.org/ns/shacl#");
    let plain_path = issue.path.as_deref().is_some_and(|p| {
        p.starts_with("http://") || p.starts_with("https://") || p.starts_with("urn:")
    });
    LOCAL.contains(&component) && plain_path
}

#[cfg(feature = "shacl")]
fn is_violation(issue: &crate::shacl::ValidationIssue) -> bool {
    issue.severity.to_ascii_lowercase().contains("violation")
}

/// Identity of a violation for debt matching: stricter than [`issue_key`].
///
/// Adds the message and the source shape, INCLUDING an anonymous one. Both
/// runs it compares validate the same shapes text through the one cached
/// validator, so a blank shape keeps its label between them; dropping it would
/// let two different anonymous shapes on the same path share a key and admit
/// a new violation as debt. If a label did differ (a cache eviction), the keys
/// differ and the write is refused: the safe direction.
#[cfg(feature = "shacl")]
fn debt_key(issue: &crate::shacl::ValidationIssue) -> String {
    format!(
        "{}|{}|{}",
        issue_key(issue),
        issue.source_shape.as_deref().unwrap_or(""),
        issue.message.as_deref().unwrap_or("")
    )
}

/// Paths a delete can break: those with `sh:minCount >= 1` (see
/// [`required_paths`]) plus those with an `sh:hasValue`.
#[cfg(feature = "shacl")]
fn deletion_sensitive_paths(shapes_turtle: &str) -> Result<BTreeSet<String>> {
    const SH_PATH: &str = "http://www.w3.org/ns/shacl#path";
    const SH_HAS_VALUE: &str = "http://www.w3.org/ns/shacl#hasValue";
    let parser = oxttl::TurtleParser::new()
        .with_base_iri("http://example.org/")
        .map_err(|e| Error::InvalidValue(format!("shapes base IRI: {e}")))?;
    let mut path_of: BTreeMap<String, String> = BTreeMap::new();
    let mut valued: BTreeSet<String> = BTreeSet::new();
    for triple in parser.for_reader(shapes_turtle.as_bytes()) {
        let triple = triple.map_err(|e| Error::InvalidValue(format!("shapes parse error: {e}")))?;
        let subject = triple.subject.to_string();
        match triple.predicate.as_str() {
            SH_PATH => {
                if let OxTerm::NamedNode(path) = &triple.object {
                    path_of.insert(subject, path.as_str().to_string());
                }
            }
            SH_HAS_VALUE => {
                valued.insert(subject);
            }
            _ => {}
        }
    }
    let mut paths = required_paths(shapes_turtle)?;
    paths.extend(valued.into_iter().filter_map(|s| path_of.get(&s).cloned()));
    Ok(paths)
}

/// Identity of a post-state issue across two validations of one data set.
#[cfg(feature = "shacl")]
pub(crate) fn issue_identity(issue: &crate::shacl::ValidationIssue) -> String {
    format!(
        "{}|{}",
        issue_key(issue),
        issue.message.as_deref().unwrap_or("")
    )
}

/// Post-state value issues the store already had before transaction `tx_id`.
///
/// The write-savepoint gate judges stored values of every touched focus, so a
/// bare `{name, type}` reuse of a node whose stored value already breaks a
/// `sh:class` or datatype rule was refused for it. The rule here is the one
/// [`admit_preexisting_debt`] applies: the focus must have existed before this
/// transaction, the component must be focus-local, and the same issue (shape
/// included) must hold on the pre-state, here the focus's facts in `graph`
/// minus this transaction's, plus ROOT types when `graph` is a named graph, as
/// the gate itself reads them. Both runs use one compiled validator. An issue
/// whose recomputed twins are not ALL debt is kept, so a collision refuses.
/// Returns the [`issue_identity`] of each admitted issue.
#[cfg(feature = "shacl")]
pub(crate) fn poststate_preexisting(
    store: &Store,
    shapes_turtle: &str,
    post_data: &str,
    issues: &[&crate::shacl::ValidationIssue],
    graph: i64,
    tx_id: i64,
) -> Result<BTreeSet<String>> {
    let focuses: BTreeSet<String> = issues
        .iter()
        .filter(|i| is_focus_local(i))
        .map(|i| i.focus_node.trim_matches(['<', '>']).to_string())
        .collect();
    let mut admitted = BTreeSet::new();
    if focuses.is_empty() {
        return Ok(admitted);
    }
    let rdf_type = store.lookup(RDF_TYPE)?;
    let mut lines = BTreeSet::new();
    let mut referenced = BTreeSet::new();
    let mut existing = BTreeSet::new();
    for iri in &focuses {
        let Some(entity) = store.lookup(iri)? else {
            continue;
        };
        let mut scopes = vec![(graph, None)];
        if graph != crate::schema::ROOT_GRAPH {
            scopes.push((crate::schema::ROOT_GRAPH, rdf_type));
        }
        for (scope, only) in scopes {
            for fact in store.entity_facts_in_graph(entity, scope)? {
                if fact.tx == tx_id || only.is_some_and(|t| fact.attribute != t) {
                    continue;
                }
                let attr = store.resolve(fact.attribute)?;
                let Ok(term) = crate::rdf::value_to_term(store, &fact.value) else {
                    continue;
                };
                if let (Value::Ref(id), true) = (&fact.value, attr != RDF_TYPE) {
                    referenced.insert(store.resolve(*id)?);
                }
                lines.insert(format!("<{iri}> <{attr}> {term} .\n"));
                existing.insert(iri.clone());
            }
        }
    }
    if existing.is_empty() {
        return Ok(admitted);
    }
    let mut pre: String = lines.into_iter().collect();
    pre.push_str(&store_type_context_in_graph(store, &referenced, graph)?);

    let validator = crate::shacl::cached_validator(shapes_turtle)?;
    let debt: BTreeSet<String> = validator
        .validate(pre.as_bytes())?
        .results
        .iter()
        .filter(|r| is_violation(r) && is_focus_local(r) && existing.contains(&r.focus_node))
        .map(debt_key)
        .collect();
    let post = validator.validate(post_data.as_bytes())?;
    let graph_iri = store.graph_iri_of(graph);
    for issue in issues {
        let focus = issue.focus_node.trim_matches(['<', '>']);
        if !is_focus_local(issue) || !existing.contains(focus) {
            continue;
        }
        let identity = issue_identity(issue);
        let twins: Vec<_> = post
            .results
            .iter()
            .filter(|r| is_violation(r) && issue_identity(r) == identity)
            .collect();
        if twins.is_empty() || !twins.iter().all(|r| debt.contains(&debt_key(r))) {
            continue;
        }
        store.queue_write_event(crate::store::PendingWriteEvent {
            event_type: "shacl.debt_admitted".to_string(),
            subject: Some(focus.to_string()),
            payload: serde_json::json!({
                "shape": twins[0].source_shape,
                "message": issue.message,
                "component": issue.component,
                "path": issue.path,
                "graph": graph_iri,
                "gate": "post-state",
            }),
        });
        admitted.insert(identity);
    }
    Ok(admitted)
}

/// `issues` minus the pre-existing debt [`poststate_preexisting`] admits.
#[cfg(feature = "shacl")]
pub(crate) fn new_only<'a>(
    store: &Store,
    shapes_turtle: &str,
    post_data: &str,
    issues: Vec<&'a crate::shacl::ValidationIssue>,
    graph: i64,
    tx_id: i64,
) -> Result<Vec<&'a crate::shacl::ValidationIssue>> {
    let admitted = poststate_preexisting(store, shapes_turtle, post_data, &issues, graph, tx_id)?;
    Ok(issues
        .into_iter()
        .filter(|issue| !admitted.contains(&issue_identity(issue)))
        .collect())
}
