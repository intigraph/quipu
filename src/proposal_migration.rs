//! Data migrations carried by schema-change proposals (aegis-esd, gap G5 of
//! `docs/design/terminusdb-gap-analysis.md`).
//!
//! Accepting a proposal used to change the shapes and leave instance data
//! alone, so renaming a property or moving a class left old facts the new
//! shapes refuse or ignore. A proposal may now carry a migration program: an
//! ordered list of operations, each rendered to a SPARQL Update template.
//!
//! - `rename_predicate` — `{from, to}`: every `?s <from> ?o` becomes `?s <to> ?o`.
//! - `retype_class` — `{from, to}`: every `?s a <from>` becomes `?s a <to>`.
//! - `cast_datatype` — `{predicate, datatype}`: every literal object of
//!   `predicate` is re-typed to `datatype` (`STRDT(STR(?o), <datatype>)`).
//! - `split_property` — `{from, into: [{predicate, expression}]}`: each source
//!   value `?o` is replaced by one value per target, computed by a SPARQL
//!   expression over `?o`. A source value any expression leaves unbound is left
//!   untouched.
//! - `merge_property` — `{from: [..], to, expression?}`: without `expression`
//!   every source value moves to `to`; with one, the values `?v0`, `?v1`, … of
//!   the sources (in `from` order) are combined into one value of `to`.
//! - `sparql` — `{update}`: a raw `DELETE/INSERT … WHERE`, `INSERT DATA` or
//!   `DELETE DATA` template.
//!
//! Every operation reads the PRE-migration state, and the program's net
//! change (`(before − deletes) ∪ inserts`) is applied on accept through the
//! write gate as ONE transaction with source `migration:<proposal id>`,
//! together with the shape change itself. Old facts are superseded (their
//! valid time closed at the decision instant), never rewritten, so as-of reads
//! before the migration still return them. Templates read and write the ROOT
//! graph only.

use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::store::Store;

#[path = "proposal_migration_apply.rs"]
mod apply;
#[path = "proposal_migration_guard.rs"]
mod guard;
#[path = "proposal_migration_lexical.rs"]
mod lexical;
#[path = "proposal_migration_plan.rs"]
mod plan;

pub use apply::{DryRun, dry_run};
pub(crate) use apply::{Issue, accept_with_migration, issue_key, refusals, target_queries};
pub use guard::{max_facts, set_max_facts};
pub use plan::{MigrationPlan, OpReport, plan};

const RDF_TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";

/// The transaction source a proposal's migration commits under.
#[must_use]
pub fn migration_source(proposal_id: i64) -> String {
    format!("migration:{proposal_id}")
}

/// One target of a `split_property` operation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SplitTarget {
    /// The predicate the computed value is written under.
    pub predicate: String,
    /// A SPARQL expression over `?o`, the source value.
    pub expression: String,
}

/// One operation of a migration program.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum MigrationOp {
    /// Move every value of `from` to `to`.
    RenamePredicate { from: String, to: String },
    /// Replace the `rdf:type` `from` with `to`.
    RetypeClass { from: String, to: String },
    /// Re-type every literal value of `predicate` to `datatype`.
    CastDatatype { predicate: String, datatype: String },
    /// Replace each value of `from` with one computed value per target.
    SplitProperty {
        from: String,
        into: Vec<SplitTarget>,
    },
    /// Move (or combine with `expression`) the values of `from` into `to`.
    MergeProperty {
        from: Vec<String>,
        to: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expression: Option<String>,
    },
    /// A raw SPARQL Update template.
    Sparql { update: String },
}

fn iri(value: &str) -> Result<String> {
    oxrdf::NamedNode::new(value)
        .map(|n| format!("<{}>", n.as_str()))
        .map_err(|e| Error::InvalidValue(format!("migration IRI {value:?} is invalid: {e}")))
}

impl MigrationOp {
    /// The operation's name, as written in the program.
    #[must_use]
    pub fn name(&self) -> &'static str {
        match self {
            Self::RenamePredicate { .. } => "rename_predicate",
            Self::RetypeClass { .. } => "retype_class",
            Self::CastDatatype { .. } => "cast_datatype",
            Self::SplitProperty { .. } => "split_property",
            Self::MergeProperty { .. } => "merge_property",
            Self::Sparql { .. } => "sparql",
        }
    }

    /// How many update operations this typed operation renders to; `None`
    /// for a raw `sparql` template.
    fn expected_operations(&self) -> Option<usize> {
        match self {
            Self::MergeProperty {
                from,
                expression: None,
                ..
            } => Some(from.len()),
            Self::Sparql { .. } => None,
            _ => Some(1),
        }
    }

    /// [`Self::render`], checked: the template must pass the ROOT-only
    /// template rules, and a typed operation must render exactly the
    /// operations it stands for.
    pub(crate) fn checked_render(&self) -> Result<String> {
        let update = self.render()?;
        let count = plan::check_template(&update)?;
        if self.expected_operations().is_some_and(|n| n != count) {
            return Err(Error::InvalidValue(format!(
                "{} rendered {count} update operations, expected {}",
                self.name(),
                self.expected_operations().unwrap_or_default()
            )));
        }
        Ok(update)
    }

    /// The SPARQL Update this operation runs.
    pub fn render(&self) -> Result<String> {
        Ok(match self {
            Self::RenamePredicate { from, to } => {
                let (from, to) = (iri(from)?, iri(to)?);
                format!(
                    "DELETE {{ ?s {from} ?o }} INSERT {{ ?s {to} ?o }} WHERE {{ ?s {from} ?o }}"
                )
            }
            Self::RetypeClass { from, to } => {
                let (from, to, ty) = (iri(from)?, iri(to)?, iri(RDF_TYPE)?);
                format!(
                    "DELETE {{ ?s {ty} {from} }} INSERT {{ ?s {ty} {to} }} WHERE {{ ?s {ty} {from} }}"
                )
            }
            Self::CastDatatype {
                predicate,
                datatype,
            } => {
                let (p, dt) = (iri(predicate)?, iri(datatype)?);
                format!(
                    "DELETE {{ ?s {p} ?o }} INSERT {{ ?s {p} ?cast }} \
                     WHERE {{ ?s {p} ?o FILTER(isLiteral(?o)) BIND(STRDT(STR(?o), {dt}) AS ?cast) }}"
                )
            }
            Self::SplitProperty { from, into } => {
                if into.is_empty() {
                    return Err(Error::InvalidValue(
                        "split_property needs at least one target in 'into'".into(),
                    ));
                }
                let from = iri(from)?;
                let (mut insert, mut binds, mut bound) = (Vec::new(), Vec::new(), Vec::new());
                for (i, target) in into.iter().enumerate() {
                    insert.push(format!("?s {} ?split{i}", iri(&target.predicate)?));
                    binds.push(format!(
                        "BIND(({}) AS ?split{i})",
                        guard::expression(&target.expression)?
                    ));
                    bound.push(format!("BOUND(?split{i})"));
                }
                format!(
                    "DELETE {{ ?s {from} ?o }} INSERT {{ {} }} WHERE {{ ?s {from} ?o {} FILTER({}) }}",
                    insert.join(" . "),
                    binds.join(" "),
                    bound.join(" && ")
                )
            }
            Self::MergeProperty {
                from,
                to,
                expression,
            } => {
                if from.is_empty() {
                    return Err(Error::InvalidValue(
                        "merge_property needs at least one source in 'from'".into(),
                    ));
                }
                let to = iri(to)?;
                match expression {
                    None => from
                        .iter()
                        .map(|p| {
                            let p = iri(p)?;
                            Ok(format!(
                                "DELETE {{ ?s {p} ?o }} INSERT {{ ?s {to} ?o }} WHERE {{ ?s {p} ?o }}"
                            ))
                        })
                        .collect::<Result<Vec<_>>>()?
                        .join(" ;\n"),
                    Some(expression) => {
                        let expression = guard::expression(expression)?;
                        let patterns = from
                            .iter()
                            .enumerate()
                            .map(|(i, p)| Ok(format!("?s {} ?v{i}", iri(p)?)))
                            .collect::<Result<Vec<_>>>()?
                            .join(" . ");
                        format!(
                            "DELETE {{ {patterns} }} INSERT {{ ?s {to} ?merged }} \
                             WHERE {{ {patterns} BIND(({expression}) AS ?merged) FILTER(BOUND(?merged)) }}"
                        )
                    }
                }
            }
            Self::Sparql { update } => update.clone(),
        })
    }
}

/// Parse a migration program from its JSON form (an array of operations) and
/// check every template before anything is stored.
pub fn parse_program(value: &serde_json::Value) -> Result<Vec<MigrationOp>> {
    let program: Vec<MigrationOp> = serde_json::from_value(value.clone())
        .map_err(|e| Error::InvalidValue(format!("migration program is invalid: {e}")))?;
    if program.is_empty() {
        return Err(Error::InvalidValue(
            "migration program is empty; omit 'migration' instead".into(),
        ));
    }
    for (index, op) in program.iter().enumerate() {
        op.checked_render().map_err(|e| {
            Error::InvalidValue(format!("migration op {index} ({}): {e}", op.name()))
        })?;
    }
    Ok(program)
}

impl Store {
    /// Attach a migration program to a pending proposal.
    pub fn set_proposal_migration(&self, proposal_id: i64, program: &[MigrationOp]) -> Result<()> {
        let json = serde_json::to_string(program)
            .map_err(|e| Error::InvalidValue(format!("migration program: {e}")))?;
        self.ensure_ownership_writable()?;
        // One statement binds replacement to the still-pending decision, so
        // an accepted proposal always retains the program it actually ran.
        let changed = self.conn.execute(
            "INSERT INTO proposal_migrations (proposal_id, program)
             SELECT id, ?2 FROM proposals WHERE id = ?1 AND status = 'pending'
             ON CONFLICT(proposal_id) DO UPDATE SET program = excluded.program",
            params![proposal_id, json],
        )?;
        if changed == 0 {
            return Err(Error::InvalidValue(format!(
                "proposal {proposal_id} not found or no longer pending; migration program is immutable"
            )));
        }
        Ok(())
    }

    /// The migration program a proposal carries, if any.
    pub fn get_proposal_migration(&self, proposal_id: i64) -> Result<Option<Vec<MigrationOp>>> {
        let json: Option<String> = self
            .conn
            .query_row(
                "SELECT program FROM proposal_migrations WHERE proposal_id = ?1",
                params![proposal_id],
                |row| row.get(0),
            )
            .optional()?;
        json.map(|j| {
            serde_json::from_str(&j)
                .map_err(|e| Error::InvalidValue(format!("stored migration program: {e}")))
        })
        .transpose()
    }
}

#[cfg(all(test, feature = "shacl"))]
#[path = "proposal_migration_tests.rs"]
mod tests;
