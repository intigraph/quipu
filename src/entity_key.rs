//! Key-field IRI derivation for `minting = "key"` namespaces.
//!
//! Design: `docs/design/key-field-iri-minting.md`. A keyed shape names the
//! fields that ARE the entity's identity (`quipu:keyFields (ex:host ex:port)`);
//! two writers that assert the same key values for the same type derive the
//! same IRI. Labels are never key material. This module only derives; the
//! write, convergence and refusal rules live in `entity_reference`.

use std::collections::{BTreeMap, BTreeSet};

use oxrdf::{Literal, NamedNode, Term};
use serde_json::{Map, Value as Json};
use sha2::{Digest, Sha256};

use crate::{Error, Result, namespace};

/// Ordered RDF list of predicate IRIs on a node shape.
pub const KEY_FIELDS: &str = "https://quipu.dev/ontology/keyFields";
/// `"hash"` (default) or `"lexical"`.
pub const KEY_STRATEGY: &str = "https://quipu.dev/ontology/keyStrategy";
/// Domain separator for the hash input. Changing it re-keys every entity, so a
/// new derivation gets a new version string, never an edit to this one.
const HASH_VERSION: &str = "quipu-key-v1";

const SH: &str = namespace::SHACL;
const RDF_FIRST: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#first";
const RDF_REST: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#rest";
const RDF_NIL: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#nil";

/// How a key tuple becomes the local part of an IRI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Strategy {
    /// 128-bit truncated SHA-256 of the canonical key input (default).
    Hash,
    /// Percent-encoded key values; readable, but drops datatypes and the type.
    Lexical,
}

/// The identity contract a keyed shape declares for one type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeySpec {
    /// Key predicate IRIs in declared order (order is part of the derivation).
    pub fields: Vec<String>,
    /// How the key tuple is spelled in the IRI.
    pub strategy: Strategy,
}

fn invalid(message: impl std::fmt::Display) -> Error {
    Error::InvalidValue(format!("key minting: {message}"))
}

/// Find the single keyed shape targeting `node_type` in the loaded shapes.
///
/// Refuses when there is none, more than one, a malformed list, an unknown
/// strategy, or a key field the same shape does not declare as exactly-one
/// (`sh:minCount 1` and `sh:maxCount 1`). The cardinality requirement is what
/// makes a divergent key value a merge conflict instead of a silent union.
pub fn key_spec(shapes_turtle: &str, node_type: &str) -> Result<KeySpec> {
    let parser = oxttl::TurtleParser::new()
        .with_base_iri("http://example.org/")
        .map_err(|e| invalid(format!("shapes base IRI: {e}")))?;
    // subject (N-Triples spelling) -> predicate -> objects
    let mut graph: BTreeMap<String, BTreeMap<String, Vec<Term>>> = BTreeMap::new();
    for triple in parser.for_reader(shapes_turtle.as_bytes()) {
        let triple = triple.map_err(|e| invalid(format!("shapes parse error: {e}")))?;
        graph
            .entry(triple.subject.to_string())
            .or_default()
            .entry(triple.predicate.as_str().to_string())
            .or_default()
            .push(triple.object);
    }
    let objects = |subject: &str, predicate: &str| -> Vec<Term> {
        graph
            .get(subject)
            .and_then(|p| p.get(predicate))
            .cloned()
            .unwrap_or_default()
    };
    let target = format!("{SH}targetClass");
    let keyed: Vec<&String> = graph
        .iter()
        .filter(|(_, preds)| {
            preds.contains_key(KEY_FIELDS)
                && preds.get(&target).is_some_and(|ts| {
                    ts.iter()
                        .any(|t| matches!(t, Term::NamedNode(n) if n.as_str() == node_type))
                })
        })
        .map(|(s, _)| s)
        .collect();
    let shape = match keyed.as_slice() {
        [one] => (*one).clone(),
        [] => {
            return Err(invalid(format!(
                "no shape with quipu:keyFields targets <{node_type}>"
            )));
        }
        _ => {
            return Err(invalid(format!(
                "more than one keyed shape targets <{node_type}>"
            )));
        }
    };
    let heads = objects(&shape, KEY_FIELDS);
    let [head] = heads.as_slice() else {
        return Err(invalid("quipu:keyFields must have exactly one list value"));
    };
    let mut fields = Vec::new();
    let mut node = head.to_string();
    let mut guard = 0;
    while node != format!("<{RDF_NIL}>") {
        guard += 1;
        let firsts = objects(&node, RDF_FIRST);
        let rests = objects(&node, RDF_REST);
        let ([Term::NamedNode(field)], [rest]) = (firsts.as_slice(), rests.as_slice()) else {
            return Err(invalid(
                "quipu:keyFields must be an RDF list of predicate IRIs",
            ));
        };
        if guard > 64 {
            return Err(invalid("quipu:keyFields list is too long or cyclic"));
        }
        fields.push(field.as_str().to_string());
        node = rest.to_string();
    }
    if fields.is_empty() || fields.iter().collect::<BTreeSet<_>>().len() != fields.len() {
        return Err(invalid(
            "quipu:keyFields must be a non-empty list without repeats",
        ));
    }
    let strategy = match objects(&shape, KEY_STRATEGY).as_slice() {
        [] => Strategy::Hash,
        [Term::Literal(l)] if l.value() == "hash" => Strategy::Hash,
        [Term::Literal(l)] if l.value() == "lexical" => Strategy::Lexical,
        _ => return Err(invalid("quipu:keyStrategy must be \"hash\" or \"lexical\"")),
    };
    let count_is_one = |property: &str, predicate: &str| {
        matches!(objects(property, predicate).as_slice(),
            [Term::Literal(l)] if l.value().parse::<i64>() == Ok(1))
    };
    for field in &fields {
        let declared = objects(&shape, &format!("{SH}property")).iter().any(|p| {
            let p = p.to_string();
            matches!(objects(&p, &format!("{SH}path")).as_slice(),
                [Term::NamedNode(path)] if path.as_str() == field)
                && count_is_one(&p, &format!("{SH}minCount"))
                && count_is_one(&p, &format!("{SH}maxCount"))
        });
        if !declared {
            return Err(invalid(format!(
                "key field <{field}> must be declared sh:minCount 1 and sh:maxCount 1 on the keyed shape"
            )));
        }
    }
    Ok(KeySpec { fields, strategy })
}

/// Turn the request's `keys` object into one RDF term per key field, in the
/// shape's order. Exactly the declared fields, no more and no fewer.
///
/// Values: a JSON string is a plain literal, an integer is `xsd:integer`, a
/// boolean is `xsd:boolean`, `{"@id": iri}` is an IRI, `{"@value", "@type"}`
/// a typed literal and `{"@value", "@language"}` a language-tagged one.
/// Floats and non-canonical integer/boolean spellings refuse: a key must have
/// exactly one spelling or two writers diverge.
pub fn key_terms(spec: &KeySpec, keys: &Map<String, Json>) -> Result<Vec<(String, Term)>> {
    let declared: BTreeSet<&str> = spec.fields.iter().map(String::as_str).collect();
    if let Some(extra) = keys.keys().find(|k| !declared.contains(k.as_str())) {
        return Err(invalid(format!(
            "<{extra}> is not a key field of this type"
        )));
    }
    spec.fields
        .iter()
        .map(|field| {
            let value = keys
                .get(field)
                .ok_or_else(|| invalid(format!("missing key field <{field}>")))?;
            Ok((field.clone(), json_term(value)?))
        })
        .collect()
}

fn json_term(value: &Json) -> Result<Term> {
    let term: Term = match value {
        Json::String(s) => Literal::new_simple_literal(s).into(),
        Json::Bool(b) => {
            Literal::new_typed_literal(b.to_string(), xsd(namespace::XSD_BOOLEAN)).into()
        }
        Json::Number(n) => {
            let n = n.as_i64().ok_or_else(|| {
                invalid("a numeric key must be an integer; floats have no single spelling")
            })?;
            Literal::new_typed_literal(n.to_string(), xsd(namespace::XSD_INTEGER)).into()
        }
        Json::Object(o) => match (
            o.get("@id"),
            o.get("@value"),
            o.get("@type"),
            o.get("@language"),
        ) {
            (Some(Json::String(iri)), None, None, None) if o.len() == 1 => {
                NamedNode::new(iri).map_err(invalid)?.into()
            }
            (None, Some(Json::String(lexical)), Some(Json::String(dt)), None) if o.len() == 2 => {
                canonical_lexical(lexical, dt)?;
                Literal::new_typed_literal(lexical, NamedNode::new(dt).map_err(invalid)?).into()
            }
            (None, Some(Json::String(lexical)), None, Some(Json::String(tag))) if o.len() == 2 => {
                Literal::new_language_tagged_literal(lexical, tag.to_ascii_lowercase())
                    .map_err(invalid)?
                    .into()
            }
            _ => return Err(invalid("unsupported key value object")),
        },
        _ => return Err(invalid("unsupported key value")),
    };
    if matches!(&term, Term::Literal(l) if l.value().is_empty()) {
        return Err(invalid("empty key value"));
    }
    Ok(term)
}

fn xsd(iri: &str) -> NamedNode {
    NamedNode::new_unchecked(iri)
}

fn canonical_lexical(lexical: &str, datatype: &str) -> Result<()> {
    let ok = match datatype {
        namespace::XSD_INTEGER => lexical
            .parse::<i64>()
            .is_ok_and(|n| n.to_string() == lexical),
        namespace::XSD_BOOLEAN => lexical == "true" || lexical == "false",
        namespace::XSD_DOUBLE | namespace::XSD_FLOAT | namespace::XSD_DECIMAL => false,
        _ => true,
    };
    if ok {
        Ok(())
    } else {
        Err(invalid(format!(
            "non-canonical or unkeyable lexical form for <{datatype}>"
        )))
    }
}

/// The local part of the IRI for `terms` of `node_type`.
///
/// Hash: SHA-256 over a versioned, newline-separated N-Triples rendering of the
/// type and each (field, term), truncated to 128 bits, in the same 26-character
/// base32 alphabet as opaque keys. Lexical: each value's lexical form (an IRI's
/// full spelling), percent-encoded outside RFC 3986 unreserved, joined by `+`.
pub fn derive_key(spec: &KeySpec, node_type: &str, terms: &[(String, Term)]) -> String {
    match spec.strategy {
        Strategy::Hash => {
            let mut input = format!("{HASH_VERSION}\n<{node_type}>\n");
            for (field, term) in terms {
                input.push_str(&format!("<{field}>\n{term}\n"));
            }
            let digest = Sha256::digest(input.as_bytes());
            base32_lower(&digest[..16])
        }
        Strategy::Lexical => terms
            .iter()
            .map(|(_, term)| {
                let text = match term {
                    Term::Literal(l) => l.value().to_string(),
                    Term::NamedNode(n) => n.as_str().to_string(),
                    other => other.to_string(),
                };
                text.bytes()
                    .map(|b| {
                        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
                            (b as char).to_string()
                        } else {
                            format!("%{b:02X}")
                        }
                    })
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("+"),
    }
}

/// RFC 4648 base32, lower case, no padding.
pub(crate) fn base32_lower(bytes: &[u8]) -> String {
    let alphabet = b"abcdefghijklmnopqrstuvwxyz234567";
    let mut out = String::new();
    let mut bits = 0u32;
    let mut available = 0;
    for &byte in bytes {
        bits = (bits << 8) | u32::from(byte);
        available += 8;
        while available >= 5 {
            available -= 5;
            out.push(alphabet[((bits >> available) & 31) as usize] as char);
        }
    }
    if available > 0 {
        out.push(alphabet[((bits << (5 - available)) & 31) as usize] as char);
    }
    out
}

#[cfg(test)]
#[path = "entity_key_tests.rs"]
mod tests;
