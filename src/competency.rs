//! Strict, offline authoring contract for pendant questions and commands.
//!
//! Compilation is pure: it neither opens a store nor executes a command. Share
//! carriage, runtime verification and authorization are separate consumers.
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

mod compile;
mod lint;
#[cfg(test)]
mod tests;

/// Authoring member carried by a competency-enabled pendant.
pub const FILE: &str = "competency.toml";

/// Versioned pendant contract; unknown fields and block kinds are errors.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Contract {
    /// Must be `quipu-competency/1`.
    pub schema: String,
    /// Read contracts, including projections.
    #[serde(default, rename = "question")]
    pub questions: Vec<Question>,
    /// Write contracts; linting never executes these.
    #[serde(default, rename = "command")]
    pub commands: Vec<Command>,
}

/// One named competency question.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Question {
    /// Unique lowercase identifier.
    pub id: String,
    /// The question in plain words.
    pub ask: String,
    /// Inline SPARQL, mutually exclusive with `query_ref`.
    pub query: Option<String>,
    /// Existing stored query in the pendant's `queries.ttl`.
    pub query_ref: Option<String>,
    /// Input names and scalar types (`?` marks optional).
    #[serde(default)]
    pub inputs: BTreeMap<String, String>,
    /// SELECT result columns and scalar types.
    #[serde(default)]
    pub returns: BTreeMap<String, String>,
    /// Optional RDF result shape, required for CONSTRUCT.
    pub returns_shape: Option<String>,
    /// Shape or class IRIs justified by this question.
    pub justifies: Vec<String>,
    /// Example arguments for later runtime verification.
    #[serde(default)]
    pub example: BTreeMap<String, String>,
    /// Optional row/boolean expectation.
    pub expect: Option<String>,
    /// Retired definitions remain in the contract.
    #[serde(default)]
    pub retired: bool,
}

/// One named write contract. The store, not this file, maps classes to grants.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Command {
    /// Unique lowercase identifier.
    pub id: String,
    /// What the command records.
    pub does: String,
    /// Typed input declarations.
    #[serde(default)]
    pub inputs: BTreeMap<String, String>,
    /// Turtle or SPARQL INSERT DATA with declared placeholders.
    pub template: String,
    /// Shape/policy IRIs to satisfy at execution time.
    pub satisfies: Vec<String>,
    /// Free-form authorization class; no built-in grants.
    pub write_class: String,
    /// Required input names forming the idempotency key.
    pub key: Vec<String>,
    /// Example arguments for later staged verification.
    pub example: BTreeMap<String, String>,
    /// Retain retired commands as contract data.
    #[serde(default)]
    pub retired: bool,
}

/// Deterministically compiled RDF members. This does not write files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Compiled {
    /// Extended `StoredQuery` definitions.
    pub queries: String,
    /// Command definitions (data, never executable on import).
    pub commands: String,
}

pub(super) fn invalid(message: impl Into<String>) -> Error {
    Error::InvalidValue(format!("competency: {}", message.into()))
}

pub(super) fn identifier(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && s.bytes().next().is_some_and(|b| b != b'-')
        && !s.ends_with('-')
}

pub(super) fn iri(s: &str) -> Result<()> {
    oxrdf::NamedNode::new(s).map_err(|e| invalid(format!("invalid IRI {s:?}: {e}")))?;
    Ok(())
}

/// Parse the schema and validate every definition offline.
///
/// `registry` supplies read-only definitions for `query_ref`; no live lookup is
/// performed. Missing references are errors, never silently skipped.
///
/// # Errors
/// Schema, type, duplicate identity, unresolved reference or syntax errors.
pub fn parse(text: &str, registry: &[crate::share_queries::SharedQuery]) -> Result<Contract> {
    let contract: Contract = toml::from_str(text).map_err(|e| invalid(e.to_string()))?;
    contract.lint(registry)?;
    Ok(contract)
}

/// Load and lint the authoring file plus any referenced queries from a directory.
///
/// # Errors
/// File errors or any [`parse`] refusal.
pub fn load(pendant: &Path) -> Result<Contract> {
    let text = std::fs::read_to_string(pendant.join(FILE))
        .map_err(|e| invalid(format!("cannot read {FILE}: {e}")))?;
    let raw: Contract = toml::from_str(&text).map_err(|e| invalid(e.to_string()))?;
    let registry = if raw.questions.iter().any(|q| q.query_ref.is_some()) {
        let ttl = std::fs::read_to_string(pendant.join(crate::share_queries::QUERIES_FILE))
            .map_err(|e| invalid(format!("cannot resolve query_ref: {e}")))?;
        crate::share_queries::from_turtle(&ttl)?
    } else {
        Vec::new()
    };
    raw.lint(&registry)?;
    Ok(raw)
}

impl Contract {
    /// Validate all blocks, including retired ones, without running them.
    ///
    /// # Errors
    /// Invalid schema or definition, with the block identity in the error.
    pub fn lint(&self, registry: &[crate::share_queries::SharedQuery]) -> Result<()> {
        if self.schema != "quipu-competency/1" {
            return Err(invalid(format!("unsupported schema {:?}", self.schema)));
        }
        let mut ids = BTreeSet::new();
        for id in self
            .questions
            .iter()
            .map(|q| &q.id)
            .chain(self.commands.iter().map(|c| &c.id))
        {
            if !identifier(id) || !ids.insert(id) {
                return Err(invalid(format!("invalid or duplicate id {id:?}")));
            }
        }
        for q in &self.questions {
            lint::question(q, registry).map_err(|e| invalid(format!("question {}: {e}", q.id)))?;
        }
        for c in &self.commands {
            lint::command(c).map_err(|e| invalid(format!("command {}: {e}", c.id)))?;
        }
        Ok(())
    }
}
