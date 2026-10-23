//! Refuse recursive SHACL shape graphs (aegis-ahu0).
//!
//! The validator follows `sh:node` (and the other shape-referencing
//! constraints) by recursion, so a shape graph with a cycle walks a linked data
//! chain one stack frame per hop. Over a long enough chain that is a stack
//! overflow, and a Rust stack overflow ABORTS the process rather than
//! unwinding: one write carrying a recursive per-request shape could take the
//! whole server down.
//!
//! Without a cycle in the shape-reference graph, nesting is bounded by the
//! depth of the shape graph itself, whatever the data. That depth must be
//! bounded too: compiling a shape graph recurses once per reference, so a deep
//! enough ACYCLIC chain overflows the stack the same way, before any data is
//! seen. Both are refused. SHACL leaves the semantics of recursive shapes to
//! the implementation, so refusing them is within the spec; it is checked
//! wherever shapes enter (the validator, and shape loading), never by trial
//! validation.

use std::collections::{BTreeMap, BTreeSet};

use oxrdf::{NamedOrBlankNode, Term};

use crate::{Error, Result};

const SH: &str = "http://www.w3.org/ns/shacl#";
const RDF_FIRST: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#first";
const RDF_REST: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#rest";

/// The deepest shape-reference chain accepted (counted in shapes, the root
/// included; a property shape is a level of its own). Compiling recurses once
/// per level and must stay inside the smallest stack a validation runs on (a
/// 2 MiB worker thread) in a debug build, which has the largest frames: a
/// plain `sh:node` chain overflows there between 80 and 96 levels. Real shape
/// sets nest at most 4, so 16 leaves a wide margin on both sides.
pub const MAX_SHAPE_DEPTH: usize = 16;

/// Constraints whose object is ONE shape the validator evaluates in turn.
const SINGLE: &[&str] = &["node", "property", "qualifiedValueShape", "not"];
/// Constraints whose object is an RDF LIST of shapes.
const LISTS: &[&str] = &["and", "or", "xone"];

fn key_of_subject(s: &NamedOrBlankNode) -> String {
    match s {
        NamedOrBlankNode::NamedNode(n) => n.as_str().to_string(),
        NamedOrBlankNode::BlankNode(b) => format!("_:{}", b.as_str()),
    }
}

// Not a `match`: the set of `Term` variants depends on oxrdf's features (an
// RDF 1.2 triple term exists only in some builds), so a wildcard arm is either
// single-variant or required, depending on the build.
fn key_of_term(t: &Term) -> Option<String> {
    if let Term::NamedNode(n) = t {
        return Some(n.as_str().to_string());
    }
    if let Term::BlankNode(b) = t {
        return Some(format!("_:{}", b.as_str()));
    }
    None
}

/// The first cycle in the shape-reference graph of `shapes_turtle`, as the
/// list of shape nodes it passes through (first node repeated at the end), or
/// `None` when the graph is acyclic.
pub fn find_shape_cycle(shapes_turtle: &str) -> Result<Option<Vec<String>>> {
    Ok(first_cycle(&reference_edges(shapes_turtle)?))
}

/// The length (in shapes) of the deepest shape-reference chain in an ACYCLIC
/// `shapes_turtle`, with the shape it starts from; `None` when it holds no
/// reference at all. Call only after [`find_shape_cycle`] returned `None`.
pub fn deepest_shape_chain(shapes_turtle: &str) -> Result<Option<(usize, String)>> {
    Ok(deepest(&reference_edges(shapes_turtle)?))
}

/// The shape-reference graph: each shape to the shapes it evaluates in turn.
fn reference_edges(shapes_turtle: &str) -> Result<BTreeMap<String, BTreeSet<String>>> {
    let parser = oxttl::TurtleParser::new()
        .with_base_iri("http://quipu.local/shapes")
        .map_err(|e| Error::InvalidValue(format!("shapes base IRI: {e}")))?;

    let mut edges: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut lists: Vec<(String, String)> = Vec::new();
    let mut first: BTreeMap<String, String> = BTreeMap::new();
    let mut rest: BTreeMap<String, String> = BTreeMap::new();

    for triple in parser.for_reader(shapes_turtle.as_bytes()) {
        let triple = triple.map_err(|e| Error::InvalidValue(format!("SHACL parse error: {e}")))?;
        let subject = key_of_subject(&triple.subject);
        let Some(object) = key_of_term(&triple.object) else {
            continue;
        };
        let predicate = triple.predicate.as_str();
        if predicate == RDF_FIRST {
            first.insert(subject, object);
        } else if predicate == RDF_REST {
            rest.insert(subject, object);
        } else if let Some(local) = predicate.strip_prefix(SH) {
            if SINGLE.contains(&local) {
                edges.entry(subject).or_default().insert(object);
            } else if LISTS.contains(&local) {
                lists.push((subject, object));
            }
        }
    }

    // Each list member is a shape the owner evaluates. The walk is bounded by
    // a visited set, so a malformed (cyclic) RDF list cannot loop.
    for (owner, head) in lists {
        let mut cell = Some(head);
        let mut seen = BTreeSet::new();
        while let Some(c) = cell {
            if !seen.insert(c.clone()) {
                break;
            }
            if let Some(member) = first.get(&c) {
                edges
                    .entry(owner.clone())
                    .or_default()
                    .insert(member.clone());
            }
            cell = rest.get(&c).cloned();
        }
    }

    Ok(edges)
}

/// Longest chain over an acyclic graph, iteratively (post-order: a node's
/// depth is one more than its deepest successor's).
fn deepest(edges: &BTreeMap<String, BTreeSet<String>>) -> Option<(usize, String)> {
    let mut depth: BTreeMap<&str, usize> = BTreeMap::new();
    let mut best: Option<(usize, String)> = None;
    for root in edges.keys() {
        if depth.contains_key(root.as_str()) {
            continue;
        }
        // (node, successors expanded yet)
        let mut stack: Vec<(&str, bool)> = vec![(root.as_str(), false)];
        while let Some((node, expanded)) = stack.pop() {
            if depth.contains_key(node) {
                continue;
            }
            let succ = edges.get(node);
            if expanded {
                let d = 1 + succ
                    .into_iter()
                    .flatten()
                    .map(|s| depth.get(s.as_str()).copied().unwrap_or(1))
                    .max()
                    .unwrap_or(0);
                depth.insert(node, d);
            } else {
                stack.push((node, true));
                for s in succ.into_iter().flatten() {
                    if !depth.contains_key(s.as_str()) {
                        stack.push((s.as_str(), false));
                    }
                }
            }
        }
        let d = depth[root.as_str()];
        if best.as_ref().is_none_or(|(b, _)| d > *b) {
            best = Some((d, root.clone()));
        }
    }
    best
}

/// Iterative DFS (this module exists because of unbounded recursion, so it
/// must not recurse itself).
fn first_cycle(edges: &BTreeMap<String, BTreeSet<String>>) -> Option<Vec<String>> {
    #[derive(Clone, Copy, PartialEq)]
    enum Mark {
        Open,
        Done,
    }
    let mut mark: BTreeMap<&str, Mark> = BTreeMap::new();
    for root in edges.keys() {
        if mark.contains_key(root.as_str()) {
            continue;
        }
        // Stack of (node, its successors not yet explored); `path` mirrors it.
        let mut stack: Vec<(&str, Vec<&str>)> = Vec::new();
        let mut path: Vec<&str> = Vec::new();
        let successors = |n: &str| -> Vec<&str> {
            edges
                .get(n)
                .map(|s| s.iter().map(String::as_str).collect())
                .unwrap_or_default()
        };
        mark.insert(root, Mark::Open);
        stack.push((root, successors(root)));
        path.push(root);
        while let Some((node, pending)) = stack.last_mut() {
            let node = *node;
            let next = pending.pop();
            match next {
                Some(next) => match mark.get(next) {
                    Some(Mark::Open) => {
                        let start = path.iter().position(|p| *p == next).unwrap_or(0);
                        let mut cycle: Vec<String> =
                            path[start..].iter().map(|p| (*p).to_string()).collect();
                        cycle.push(next.to_string());
                        return Some(cycle);
                    }
                    Some(Mark::Done) => {}
                    None => {
                        mark.insert(next, Mark::Open);
                        stack.push((next, successors(next)));
                        path.push(next);
                    }
                },
                None => {
                    mark.insert(node, Mark::Done);
                    stack.pop();
                    path.pop();
                }
            }
        }
    }
    None
}

/// `Err` naming the cycle when `shapes_turtle` contains recursive shapes, or
/// the root of the chain when its references nest deeper than
/// [`MAX_SHAPE_DEPTH`].
pub fn refuse_cyclic_shapes(shapes_turtle: &str) -> Result<()> {
    let edges = reference_edges(shapes_turtle)?;
    if let Some(cycle) = first_cycle(&edges) {
        return Err(Error::InvalidValue(format!(
            "recursive SHACL shapes are not supported (a shape reference cycle can \
             recurse once per data hop and overflow the stack): {}",
            cycle.join(" -> ")
        )));
    }
    match deepest(&edges) {
        Some((d, root)) if d > MAX_SHAPE_DEPTH => Err(Error::InvalidValue(format!(
            "SHACL shape references nest {d} levels deep from {root}; at most \
             {MAX_SHAPE_DEPTH} are supported (compiling recurses once per level)"
        ))),
        _ => Ok(()),
    }
}

#[cfg(test)]
#[path = "shacl_cycles_tests.rs"]
mod tests;
