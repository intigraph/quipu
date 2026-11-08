//! Typed-name admission for the existing strict-mode resolver.
//! A match is a request for a decision, never an automatic identity merge.
use std::sync::OnceLock;

use super::{LabelIndex, id_list, recorded_distinct_from};
use crate::{Result, Store, episode, namespace, types::Value};

pub(super) fn needs_check(node: &episode::Node) -> bool {
    static OPAQUE: OnceLock<regex::Regex> = OnceLock::new();
    let opaque = OPAQUE.get_or_init(|| {
        regex::Regex::new(r"(?i)^(?:(?:commit-|stmt_)?[0-9a-f]{7,64}|main-watch-[0-9a-f]{40}(?:[0-9a-f]{24})?|commit/[^\s]+/[0-9a-f]{7,64})$")
            .expect("constant opaque identifier pattern")
    });
    static WORK_ID: OnceLock<regex::Regex> = OnceLock::new();
    let work_id = WORK_ID.get_or_init(|| {
        regex::Regex::new(r"(?i)^[a-z][a-z0-9]*-[a-z0-9][a-z0-9.\-]*$")
            .expect("constant work identifier pattern")
    });
    let work = matches!(
        node.node_type
            .as_deref()
            .map(episode::node_type::local_name),
        Some("WorkItem" | "Bead")
    ) && work_id.is_match(&node.name);
    !node.name.starts_with("code/") && !opaque.is_match(&node.name) && !work
}

fn container_id(name: &str) -> Option<String> {
    static CT: OnceLock<regex::Regex> = OnceLock::new();
    let re = CT.get_or_init(|| {
        regex::Regex::new(r"(?i)^ct[-_ ]?(\d+)(?:[-_ ][a-z][a-z0-9._-]*)?$")
            .expect("constant container identity pattern")
    });
    let captures = re.captures(name.trim())?;
    let digits = captures.get(1)?.as_str().trim_start_matches('0');
    Some(if digits.is_empty() { "0" } else { digits }.to_owned())
}

fn near(a: &str, b: &str, kind: &str, threshold: f64) -> bool {
    if matches!(kind, "LXCContainer" | "Container" | "Host")
        && let (Some(a), Some(b)) = (container_id(a), container_id(b))
    {
        return a == b;
    }
    strsim::jaro_winkler(&a.to_lowercase(), &b.to_lowercase()) >= threshold
}

fn facts(store: &Store, iri: &str) -> Result<Vec<(String, Value)>> {
    let ids = store.lookup_all(iri)?;
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let attrs = [namespace::RDF_TYPE, namespace::RDFS_LABEL]
        .iter()
        .map(|p| store.lookup_all(p))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    if attrs.is_empty() {
        return Ok(Vec::new());
    }
    let sql = format!(
        "SELECT a,v FROM {} WHERE e IN ({}) AND a IN ({}) AND op=1 AND valid_to IS NULL",
        store.facts_source(),
        id_list(&ids),
        id_list(&attrs)
    );
    let mut stmt = store.conn.prepare(&sql)?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Vec<u8>>(1)?)))?;
    rows.map(|row| {
        let (a, v) = row?;
        Ok((store.resolve(a)?, Value::from_bytes(&v)?))
    })
    .collect()
}

/// Check the composed label index outside the writer. The existing generation
/// fence must rerun this pass if another transaction changes the read evidence.
pub(super) fn refusal(
    store: &Store,
    nodes: &[episode::Node],
    base: &str,
    threshold: f64,
) -> Result<Option<String>> {
    let index = LabelIndex::build(store)?;
    for (position, node) in nodes.iter().enumerate().filter(|(_, n)| needs_check(n)) {
        crate::time::check_request_deadline()?;
        let mine = episode::address::resolved_node_iri(node, base);
        // Updating/retyping an existing IRI is not minting a spelling variant.
        if !facts(store, &mine)?.is_empty() {
            continue;
        }
        let raw_kind = node.node_type.as_deref().unwrap_or("");
        let kind = episode::node_type::local_name(raw_kind);
        let type_iri = episode::node_type::node_type_iri(raw_kind, base);
        let mut excused = recorded_distinct_from(store, &mine)?;
        if !node.distinct_from.is_empty() {
            let reason = node
                .properties
                .as_ref()
                .and_then(|p| p.get("distinctReason"))
                .and_then(serde_json::Value::as_str)
                .map_or("", str::trim);
            if reason.is_empty() {
                return Ok(Some(format!(
                    "entity resolution: '{}' declares distinct_from without a nonempty properties.distinctReason",
                    node.name
                )));
            }
            excused.extend(node.distinct_from.iter().cloned());
        }
        // No top-k truncation before type filtering: unrelated kinds must not
        // crowd the compatible match out of admission.
        for candidate in index.entity_candidates(store, &node.name, threshold)? {
            if candidate.iri == mine || excused.contains(&candidate.iri) {
                continue;
            }
            let context = facts(store, &candidate.iri)?;
            let mut compatible = node.node_type.is_none();
            for (attribute, value) in &context {
                if attribute == namespace::RDF_TYPE
                    && let Value::Ref(id) = value
                {
                    compatible |= store.resolve(*id)? == type_iri;
                }
            }
            if compatible && context.iter().any(|(a, v)| {
                a == namespace::RDFS_LABEL
                    && matches!(v, Value::Str(label) if near(&node.name, label, kind, threshold))
            }) {
                return Ok(Some(message(&node.name, &candidate.iri)));
            }
        }
        // Detect aliases submitted together before either one exists.
        for other in &nodes[position + 1..] {
            let iri = episode::address::resolved_node_iri(other, base);
            if (other
                .node_type
                .as_deref()
                .map(|kind| episode::node_type_iri(kind, base))
                == Some(type_iri.clone())
                || node.node_type.is_none()
                || other.node_type.is_none())
                && iri != mine
                && !excused.contains(&iri)
                && near(&node.name, &other.name, kind, threshold)
            {
                return Ok(Some(message(&node.name, &iri)));
            }
        }
    }
    Ok(None)
}

fn message(name: &str, iri: &str) -> String {
    format!(
        "entity resolution: new typed entity '{name}' near-matches compatible entity <{iri}>; reuse the existing IRI or record a reasoned distinct decision before writing"
    )
}
