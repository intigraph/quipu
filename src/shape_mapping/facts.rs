//! Deterministic RDF projection for the protected mapping writer.
//!
//! Encoding is pure: it does not intern terms, create graphs, authorize a caller
//! or commit a transaction. The writer must supply its actual allocated tx and
//! verified authority after acquiring the store's writer reservation.

use oxrdf::{Literal, NamedNode, Term, Triple};

use super::{ShapeMapping, ShapePin, invalid, iri};
use crate::Result;

/// Mapping vocabulary, shared by projection and the shipped SHACL shapes.
pub const NS: &str = "https://quipu.dev/ontology/";
const RDF_TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";
const XSD_INTEGER: &str = "http://www.w3.org/2001/XMLSchema#integer";
const XSD_DATETIME: &str = "http://www.w3.org/2001/XMLSchema#dateTime";

/// Status changes do not alter the content digest or silently change pins.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MappingStatus {
    /// Only explicit decision/application makes a mapping effective.
    Decided,
    /// A pinned version was closed; the original pins remain evidence.
    Flagged,
    /// The validity interval has been closed by the protected retire path.
    Retired,
}

impl MappingStatus {
    /// Stable RDF status spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Decided => "decided",
            Self::Flagged => "flagged",
            Self::Retired => "retired",
        }
    }
}

/// Evidence supplied by the executing writer, never inferred from row authors.
pub(crate) struct Settlement<'a> {
    pub actor: &'a str,
    pub authority: &'a str,
    pub timestamp: &'a str,
    pub tx: i64,
}

impl Settlement<'_> {
    fn validate(&self) -> Result<()> {
        if !iri(self.actor)
            || !iri(self.authority)
            || self.tx <= 0
            || crate::time::epoch_of_rfc3339(self.timestamp).is_none()
        {
            return Err(invalid(
                "actual transaction, actor, time and authority required",
            ));
        }
        Ok(())
    }
}

fn node(iri: &str) -> Result<NamedNode> {
    NamedNode::new(iri).map_err(|_| invalid("invalid projection IRI"))
}

fn triple(out: &mut String, subject: &str, predicate: &str, object: Term) -> Result<()> {
    let value = Triple::new(node(subject)?, node(predicate)?, object);
    out.push_str(&value.to_string());
    out.push_str(" .\n");
    Ok(())
}

fn text(out: &mut String, subject: &str, slot: &str, value: &str) -> Result<()> {
    triple(
        out,
        subject,
        &format!("{NS}{slot}"),
        Literal::new_simple_literal(value).into(),
    )
}

fn link(out: &mut String, subject: &str, slot: &str, value: &str) -> Result<()> {
    triple(out, subject, &format!("{NS}{slot}"), node(value)?.into())
}

fn typed(out: &mut String, subject: &str, slot: &str, value: &str, datatype: &str) -> Result<()> {
    triple(
        out,
        subject,
        &format!("{NS}{slot}"),
        Literal::new_typed_literal(value, node(datatype)?).into(),
    )
}

fn child(mapping: &ShapeMapping, digest: &str, kind: &str, index: usize) -> String {
    // Child identity includes the whole mapping identity and content digest;
    // reusing a mapping IRI with a new decision cannot alias its prior rows.
    let identity = crate::share::sha256(mapping.id.as_bytes());
    format!(
        "urn:quipu:mapping:{kind}:{}:{}:{index}",
        &identity[7..],
        &digest[7..]
    )
}

/// The accepted decision is named by its exact mapping identity and content.
/// It is projected by the lifecycle writer, never accepted as a caller's
/// arbitrary `onAuthority` string.
pub(crate) fn decision_iri(mapping: &ShapeMapping, action: &str, tx: i64) -> Result<String> {
    if !matches!(action, "apply" | "retire" | "flag") || tx <= 0 {
        return Err(invalid("unknown decision action or transaction"));
    }
    let identity = crate::share::sha256(mapping.id.as_bytes());
    let digest = mapping.digest()?;
    Ok(format!(
        "urn:quipu:mapping:decision:{}:{}:{action}:{tx}",
        &identity[7..],
        &digest[7..]
    ))
}

pub(crate) fn decision(
    mapping: &ShapeMapping,
    at: &Settlement<'_>,
    action: &str,
) -> Result<String> {
    mapping.validate(true)?;
    at.validate()?;
    let id = decision_iri(mapping, action, at.tx)?;
    if at.authority != id
        || (action == "apply" && mapping.rows.iter().any(|row| row.author_id != at.actor))
    {
        return Err(invalid(
            "decision author or exact decision identity mismatch",
        ));
    }
    let mut out = String::new();
    triple(
        &mut out,
        &id,
        RDF_TYPE,
        node(&format!("{NS}MappingDecision"))?.into(),
    )?;
    link(&mut out, &id, "mapping", &mapping.id)?;
    link(&mut out, &id, "decidedBy", at.actor)?;
    text(&mut out, &id, "mappingDigest", &mapping.digest()?)?;
    text(&mut out, &id, "mappingAction", action)?;
    typed(
        &mut out,
        &id,
        "decidedAtTx",
        &at.tx.to_string(),
        XSD_INTEGER,
    )?;
    Ok(out)
}

fn pin(out: &mut String, id: &str, value: &ShapePin) -> Result<()> {
    triple(
        out,
        id,
        RDF_TYPE,
        node(&format!("{NS}ShapeVersionPin"))?.into(),
    )?;
    text(
        out,
        id,
        "registryKind",
        match value.registry {
            super::RegistryKind::Shapes => "shapes",
            super::RegistryKind::Ontologies => "ontologies",
        },
    )?;
    text(out, id, "shapeName", &value.name)?;
    typed(out, id, "shapeValidFrom", &value.valid_from, XSD_DATETIME)?;
    text(out, id, "shapeDigest", &value.digest)
}

/// Project an exact decided definition. Status/retirement updates belong to the
/// atomic lifecycle writer; no schema correspondence is asserted as an axiom.
pub(crate) fn definition(mapping: &ShapeMapping, at: &Settlement<'_>) -> Result<String> {
    mapping.validate(true)?;
    at.validate()?;
    let digest = mapping.digest()?;
    let mut out = String::new();
    triple(
        &mut out,
        &mapping.id,
        RDF_TYPE,
        node(&format!("{NS}ShapeMapping"))?.into(),
    )?;
    text(&mut out, &mapping.id, "mappingDigest", &digest)?;
    text(
        &mut out,
        &mapping.id,
        "mappingStatus",
        MappingStatus::Decided.as_str(),
    )?;
    link(&mut out, &mapping.id, "decidedBy", at.actor)?;
    typed(
        &mut out,
        &mapping.id,
        "decidedAtTx",
        &at.tx.to_string(),
        XSD_INTEGER,
    )?;
    for graph in &mapping.maps_from {
        link(&mut out, &mapping.id, "mapsFrom", graph)?;
    }
    for graph in &mapping.maps_to {
        link(&mut out, &mapping.id, "mapsTo", graph)?;
    }
    for (index, value) in mapping.pins.iter().enumerate() {
        let id = child(mapping, &digest, "pin", index);
        link(&mut out, &mapping.id, "pinsShape", &id)?;
        pin(&mut out, &id, value)?;
    }
    for (index, row) in mapping.rows.iter().enumerate() {
        let id = child(mapping, &digest, "row", index);
        link(&mut out, &mapping.id, "correspondence", &id)?;
        triple(
            &mut out,
            &id,
            RDF_TYPE,
            node(&format!("{NS}ShapeCorrespondence"))?.into(),
        )?;
        let sssom = crate::align::sssom::SSSOM_NS;
        for (slot, value) in [
            ("subject_id", &row.subject_id),
            ("predicate_id", &row.predicate_id),
            ("object_id", &row.object_id),
            ("author_id", &row.author_id),
        ] {
            triple(
                &mut out,
                &id,
                &format!("{sssom}{slot}"),
                node(value)?.into(),
            )?;
        }
        triple(
            &mut out,
            &id,
            &format!("{sssom}mapping_justification"),
            Literal::new_simple_literal(row.mapping_justification.as_str()).into(),
        )?;
        text(&mut out, &id, "quipu_review", &row.quipu_review)?;
    }
    Ok(out)
}

/// Project audit evidence independently of the mapping graph. The caller stages
/// this projection in the configured operations graph in the SAME transaction.
/// A flagged transition must carry the exact closed pin, not a guessed version.
pub(crate) fn record(
    mapping: &ShapeMapping,
    at: &Settlement<'_>,
    old: Option<MappingStatus>,
    new: MappingStatus,
    old_digest: Option<&str>,
    cause: Option<&ShapePin>,
) -> Result<(String, String)> {
    mapping.validate(true)?;
    at.validate()?;
    if (new == MappingStatus::Flagged) != cause.is_some()
        || cause.is_some_and(|pin| !mapping.pins.contains(pin))
        || old_digest.is_some_and(|digest| !super::valid_digest(digest))
    {
        return Err(invalid(
            "flag requires an exact closed pin from this mapping",
        ));
    }
    let digest = mapping.digest()?;
    match (old, new, old_digest) {
        (None, MappingStatus::Decided, None) | (Some(_), MappingStatus::Decided, Some(_)) => {}
        (Some(MappingStatus::Decided), MappingStatus::Flagged, Some(previous))
        | (
            Some(MappingStatus::Decided | MappingStatus::Flagged),
            MappingStatus::Retired,
            Some(previous),
        ) if previous == digest => {}
        _ => return Err(invalid("invalid lifecycle transition or old digest")),
    }
    let identity = crate::share::sha256(mapping.id.as_bytes());
    let id = format!("urn:quipu:mapping:record:{}:{}", &identity[7..], at.tx);
    let mut out = String::new();
    triple(
        &mut out,
        &id,
        RDF_TYPE,
        node(&format!("{NS}MappingRecord"))?.into(),
    )?;
    link(&mut out, &id, "mapping", &mapping.id)?;
    link(&mut out, &id, "actor", at.actor)?;
    link(&mut out, &id, "onAuthority", at.authority)?;
    typed(&mut out, &id, "tx", &at.tx.to_string(), XSD_INTEGER)?;
    typed(&mut out, &id, "time", at.timestamp, XSD_DATETIME)?;
    text(&mut out, &id, "newDigest", &digest)?;
    text(&mut out, &id, "newStatus", new.as_str())?;
    if let Some(old) = old {
        text(&mut out, &id, "oldStatus", old.as_str())?;
    }
    if let Some(old_digest) = old_digest {
        text(&mut out, &id, "oldDigest", old_digest)?;
    }
    for (index, value) in mapping.pins.iter().enumerate() {
        // A record owns its pin evidence. Do not duplicate an identical
        // (entity,predicate,value,tx) into two graphs: the existing fact log's
        // claim identity does not include g in that unique key.
        let pin_id = format!("{id}:pin:{index}");
        link(&mut out, &id, "pinsShape", &pin_id)?;
        pin(&mut out, &pin_id, value)?;
        if Some(value) == cause {
            link(&mut out, &id, "causedByShapeVersion", &pin_id)?;
        }
    }
    Ok((id, out))
}
