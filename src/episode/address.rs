//! Explicit node addresses, independent of the legacy instance/type base.
use std::collections::HashMap;

use super::Node;
use crate::{Result, error::Error};

/// Slash-bearing names are deliberate canonical paths, not labels to sanitize.
/// Keep the accepted grammar narrow so permitting `/` cannot introduce schemes,
/// traversal, query/fragment syntax, whitespace, or Turtle escapes.
pub(super) fn validate_node_name(name: &str) -> Result<()> {
    if !name.contains('/') {
        return Ok(());
    }
    let valid = !name.starts_with('/')
        && !name.ends_with('/')
        && name.split('/').all(|segment| {
            !segment.is_empty()
                && segment != "."
                && segment != ".."
                && segment
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
        });
    if valid {
        Ok(())
    } else {
        Err(crate::error::Error::InvalidValue(format!(
            "node name '{name}' is not a safe relative IRI path — slash paths must use \
             non-empty segments containing only ASCII letters, digits, '-', '_' or '.', \
             and may not contain '.' or '..' segments"
        )))
    }
}

pub(super) fn absolute_syntax(iri: &str) -> bool {
    iri.split_once(':').is_some_and(|(scheme, tail)| {
        scheme.eq_ignore_ascii_case("urn")
            || ((scheme.eq_ignore_ascii_case("https") || scheme.eq_ignore_ascii_case("http"))
                && tail.starts_with("//"))
    })
}

pub(super) fn validate_absolute(iri: &str) -> Result<()> {
    if !absolute_syntax(iri) || oxrdf::NamedNode::new(iri).is_err() {
        return Err(Error::InvalidValue(
            "explicit address must be an unmodified absolute HTTP(S) or URN IRI".into(),
        ));
    }
    Ok(())
}

pub(crate) fn resolved_node_iri(node: &Node, base: &str) -> String {
    node.iri
        .clone()
        .unwrap_or_else(|| super::node_iri(&node.name, base))
}

pub(super) fn term(node: &Node, base: &str) -> String {
    match &node.iri {
        Some(iri) => format!("<{iri}>"),
        None => super::node_term(&super::node_local_name(&node.name), base),
    }
}

/// A batch may not use an explicit address to give one subject multiple names.
/// Legacy-only spelling collisions retain their previous behavior.
pub(crate) fn validate_nodes(nodes: &[Node], base: &str) -> Result<()> {
    let mut addresses = HashMap::with_capacity(nodes.len());
    for node in nodes {
        if let Some(iri) = &node.iri {
            validate_absolute(iri)?;
        }
        let explicit = node.iri.is_some();
        if let Some(previous_explicit) = addresses.insert(resolved_node_iri(node, base), explicit)
            && (explicit || previous_explicit)
        {
            return Err(Error::InvalidValue(
                "multiple node entries claim the same explicit subject IRI".into(),
            ));
        }
    }
    Ok(())
}
