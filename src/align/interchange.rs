//! Bounded, lossless standard SSSOM input. Importing never accepts a mapping.
//!
//! This is deliberately separate from the byte-bound working `MappingSet`.
//! Unknown external justification and metadata remain evidence, not curation.
use std::collections::{BTreeMap, BTreeSet};

use oxrdf::NamedNode;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use yaml_rust2::{Yaml, YamlLoader};

use crate::error::{Error, Result};

mod metadata;
use metadata::{expand, read_metadata};

/// Input limits are checked before materializing rows or expanding YAML.
pub const MAX_BYTES: usize = 16 * 1024 * 1024;
/// A mapping file cannot request an unbounded identity projection.
pub const MAX_ROWS: usize = 50_000;
/// Maximum total materialized field and expanded identifier bytes.
pub const MAX_EXPANDED_BYTES: usize = 32 * 1024 * 1024;

/// Untrusted interchange evidence. All original bytes and fields are retained.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExternalSet {
    pub mapping_set_id: String,
    pub source_sha256: String,
    pub source: String,
    pub metadata: BTreeMap<String, Value>,
    pub rows: Vec<ExternalRow>,
}

/// A row identity includes its ordinal AND expanded content, not a claimed ID.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalRow {
    /// One-based data row, excluding metadata and the column header.
    pub ordinal: usize,
    pub fields: BTreeMap<String, String>,
    pub subject: String,
    pub predicate: String,
    pub object: String,
    pub justification: String,
    pub negated: bool,
    pub declined: bool,
    pub row_sha256: String,
}

pub(super) fn invalid(message: impl std::fmt::Display) -> Error {
    Error::InvalidValue(format!("SSSOM interchange: {message}"))
}

/// Hash a versioned canonical JSON representation (`BTreeMap` key ordering).
pub(crate) fn digest(value: &impl Serialize) -> Result<String> {
    serde_json::to_vec(value)
        .map(|bytes| crate::share::sha256(&bytes))
        .map_err(|e| Error::Serialization(e.to_string()))
}

impl ExternalRow {
    /// Relation eligibility only. This NEVER establishes local acceptance.
    #[must_use]
    pub fn identity_eligible(&self) -> bool {
        !self.negated
            && !self.declined
            && matches!(
                self.predicate.as_str(),
                "http://www.w3.org/2002/07/owl#sameAs"
                    | "http://www.w3.org/2004/02/skos/core#exactMatch"
            )
    }
}

impl ExternalSet {
    /// Verify that every derived field still corresponds to the retained bytes.
    /// Serialized evidence is not a trusted parser result.
    ///
    /// # Errors
    /// Malformed source or a changed derived field/digest.
    pub fn verify_source(&self) -> Result<()> {
        if Self::parse(&self.source)? != *self {
            return Err(invalid("serialized set differs from its original source"));
        }
        Ok(())
    }

    /// Read standard SSSOM/TSV without interpreting authors or accepted fields
    /// as local decisions. Metadata propagates to blank row slots; evidence
    /// outside the operational subset is preserved rather than guessed.
    ///
    /// # Errors
    /// Malformed/ambiguous metadata, identifiers, row widths or bounded input.
    pub fn parse(source: &str) -> Result<Self> {
        if source.len() > MAX_BYTES || source.contains('\0') {
            return Err(invalid("input exceeds 16 MiB or contains NUL"));
        }
        let mut lines = source.trim_start_matches('\u{feff}').lines().peekable();
        let mut yaml = String::new();
        while let Some(line) = lines.peek() {
            let Some(rest) = line.strip_prefix('#') else {
                break;
            };
            yaml.push_str(rest.strip_prefix(' ').unwrap_or(rest));
            yaml.push('\n');
            lines.next();
            if yaml.len() > 65_536 {
                return Err(invalid("metadata exceeds 64 KiB"));
            }
        }
        let (metadata, prefixes) = read_metadata(&yaml)?;
        let set_id = metadata
            .get("mapping_set_id")
            .and_then(Value::as_str)
            .ok_or_else(|| invalid("mapping_set_id must be a nonempty IRI"))?;
        let mapping_set_id = expand(set_id, &prefixes)?;
        let columns: Vec<_> = lines
            .next()
            .ok_or_else(|| invalid("missing TSV header"))?
            .split('\t')
            .map(str::to_owned)
            .collect();
        let unique: BTreeSet<_> = columns.iter().collect();
        if unique.len() != columns.len() || columns.iter().any(|c| c.trim() != c || c.is_empty()) {
            return Err(invalid("duplicate, empty or padded column name"));
        }
        for required in [
            "subject_id",
            "predicate_id",
            "object_id",
            "mapping_justification",
        ] {
            if !columns.iter().any(|c| c == required) && !metadata.contains_key(required) {
                return Err(invalid(format!("missing {required}")));
            }
        }
        let mut rows = Vec::new();
        let mut expanded_bytes = 0usize;
        for line in lines {
            if line.is_empty() {
                continue;
            }
            if rows.len() == MAX_ROWS {
                return Err(invalid("more than 50000 rows"));
            }
            let cells: Vec<_> = line.split('\t').collect();
            if cells.len() != columns.len() {
                return Err(invalid(format!("row {} has wrong width", rows.len() + 1)));
            }
            let mut fields: BTreeMap<String, String> = columns
                .iter()
                .cloned()
                .zip(cells.iter().map(|s| (*s).to_string()))
                .collect();
            for (key, value) in &metadata {
                let value = match value {
                    Value::String(s) => Some(s.clone()),
                    Value::Number(n) => Some(n.to_string()),
                    _ => None,
                };
                if let Some(value) = value
                    && fields.get(key).is_none_or(String::is_empty)
                {
                    fields.insert(key.clone(), value);
                }
            }
            let field = |key: &str| fields.get(key).map_or("", String::as_str);
            let subject = expand(field("subject_id"), &prefixes)?;
            let predicate = expand(field("predicate_id"), &prefixes)?;
            let object = expand(field("object_id"), &prefixes)?;
            let justification = expand(field("mapping_justification"), &prefixes)?;
            expanded_bytes = expanded_bytes.saturating_add(
                fields.iter().map(|(k, v)| k.len() + v.len()).sum::<usize>()
                    + subject.len()
                    + predicate.len()
                    + object.len()
                    + justification.len(),
            );
            if expanded_bytes > MAX_EXPANDED_BYTES {
                return Err(invalid("expanded rows exceed 32 MiB"));
            }
            let negated = match field("predicate_modifier") {
                "" => false,
                "Not" => true,
                _ => return Err(invalid("unsupported predicate_modifier")),
            };
            let declined = match field("quipu_review") {
                "" => false,
                "declined" => true,
                _ => return Err(invalid("unsupported quipu_review")),
            };
            if !field("confidence").is_empty() {
                let confidence: f64 = field("confidence")
                    .parse()
                    .map_err(|_| invalid("invalid confidence"))?;
                if !confidence.is_finite() || !(0.0..=1.0).contains(&confidence) {
                    return Err(invalid("confidence must be finite and in 0..1"));
                }
            }
            if !field("mapping_date").is_empty()
                && !super::sssom::is_xsd_date(field("mapping_date"))
            {
                return Err(invalid("mapping_date must be a real YYYY-MM-DD date"));
            }
            let row_sha256 = digest(&(
                "quipu-sssom-row-v1",
                &fields,
                &subject,
                &predicate,
                &object,
                &justification,
            ))?;
            rows.push(ExternalRow {
                ordinal: rows.len() + 1,
                fields,
                subject,
                predicate,
                object,
                justification,
                negated,
                declined,
                row_sha256,
            });
        }
        Ok(Self {
            mapping_set_id,
            source_sha256: crate::share::sha256(source.as_bytes()),
            source: source.into(),
            metadata,
            rows,
        })
    }
}

#[cfg(test)]
mod tests;
