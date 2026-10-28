//! One way to reconcile a failed merge (aegis-k7l).
//!
//! Every merge that can stop on a conflict (share reconnect, fork promote,
//! Git pendant merge) produces a decisions file whose rows are the same
//! record: a [`DecisionRecord`] plus each side's provenance, the broken rule,
//! a mechanical proposal and the operator's decision. This module is the part
//! they share:
//!
//! - [`ReconcileFile::load`] reads any of those files by its schema;
//! - [`ReconcileFile::view`] turns it into the standard [`MergeView`], which
//!   [`render_text`] and [`render_markdown`] print and which serializes as the
//!   JSON form;
//! - [`ReconcileFile::resolve`] records one choice on one row, validated
//!   against that row, without touching any store.
//!
//! Applying stays with each merge kind (`quipu merge <dir> --decisions`,
//! `quipu fork promote <name> --decisions`), so each keeps its own gates.
//! The layout is fixed so an agent can show it to a person unchanged; see
//! `docs/design/merge-reconcile.md`.

#![cfg(not(target_arch = "wasm32"))]

use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::Value as JsonValue;

use crate::error::{Error, Result};
use crate::fork_merge_decisions::{FORK_DECISIONS_SCHEMA, ForkDecisionFile, decided_values};
use crate::merge3::DecisionRecord;
use crate::share_merge_decisions::{
    DECISIONS_SCHEMA, DECISIONS_SCHEMA_V1, Decision, DecisionFile, LocalFact, Proposal, Side, union,
};
use crate::share_merge_decisions_view::{ConflictKind, kind, rule};

/// Schema of the JSON presentation.
pub const VIEW_SCHEMA: &str = "https://github.com/scbrown/quipu/merge-view/v1";

/// A decisions file of any merge kind.
#[derive(Debug, Clone, PartialEq)]
pub enum ReconcileFile {
    /// `quipu merge <share-dir> --emit-decisions` (v1 or v2).
    Share(DecisionFile),
    /// `quipu fork promote <name> --emit-decisions`.
    Fork(ForkDecisionFile),
}

/// Which surface the "next" commands are written for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Surface {
    /// `quipu ...` command lines.
    Cli,
    /// MCP tool calls.
    Mcp,
}

/// One choice for one row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Choice {
    /// Take one side (`ours`, `theirs`, `base`, `both`).
    Side(Side),
    /// Explicit N-Triples terms the person gave.
    Values(Vec<String>),
    /// Leave the row undecided; apply refuses until it is decided.
    Defer,
}

impl Choice {
    /// Parse `--choose` / `choose` plus any `--value` / `values`.
    ///
    /// # Errors
    /// An unknown choice, or values given with a side choice.
    pub fn parse(choose: Option<&str>, values: Vec<String>) -> Result<Self> {
        let side = |c: Choice| {
            if values.is_empty() {
                Ok(c)
            } else {
                Err(Error::InvalidValue(
                    "a value is given with `value`, not with a side choice".into(),
                ))
            }
        };
        match choose {
            Some("ours") => side(Choice::Side(Side::Ours)),
            Some("theirs") => side(Choice::Side(Side::Theirs)),
            Some("base") => side(Choice::Side(Side::Base)),
            Some("both") => side(Choice::Side(Side::Both)),
            Some("defer") => side(Choice::Defer),
            Some("value") | None if !values.is_empty() => Ok(Choice::Values(values)),
            Some("value") | None => Err(Error::InvalidValue(
                "choose one of ours, theirs, base, both, value (with at least one value), defer"
                    .into(),
            )),
            Some(other) => Err(Error::InvalidValue(format!(
                "unknown choice {other:?}: choose one of ours, theirs, base, both, value, defer"
            ))),
        }
    }
}

#[path = "merge_reconcile_view.rs"]
mod view;
pub use view::*;
#[path = "merge_reconcile_render.rs"]
mod render;
pub use render::{render_markdown, render_text};

impl ReconcileFile {
    /// Read a decisions file of any kind, by its `schema`.
    ///
    /// # Errors
    /// Not JSON, an unknown schema, or a file that does not match it.
    pub fn load(bytes: &[u8]) -> Result<Self> {
        let value: JsonValue = serde_json::from_slice(bytes)
            .map_err(|e| Error::InvalidValue(format!("decisions file is not JSON: {e}")))?;
        Self::from_json(value)
    }

    /// [`Self::load`] for an already-parsed value.
    ///
    /// # Errors
    /// As [`Self::load`].
    pub fn from_json(value: JsonValue) -> Result<Self> {
        let schema = value
            .get("schema")
            .and_then(JsonValue::as_str)
            .unwrap_or_default()
            .to_string();
        let bad = |e: serde_json::Error| {
            Error::InvalidValue(format!("not a valid {schema} decisions file: {e}"))
        };
        match schema.as_str() {
            DECISIONS_SCHEMA | DECISIONS_SCHEMA_V1 => {
                serde_json::from_value(value).map(Self::Share).map_err(bad)
            }
            FORK_DECISIONS_SCHEMA => serde_json::from_value(value).map(Self::Fork).map_err(bad),
            "https://github.com/scbrown/quipu/git-decisions/v1" => Err(Error::InvalidValue(
                "a Git pendant decisions.json: resolve it with `quipu pendant-resolve` for now"
                    .into(),
            )),
            other => Err(Error::InvalidValue(format!(
                "not a merge decisions file: schema {other:?}"
            ))),
        }
    }

    /// The file as JSON, as it is written back to disk.
    ///
    /// # Errors
    /// Serialization failure.
    pub fn to_json(&self) -> Result<JsonValue> {
        match self {
            Self::Share(f) => serde_json::to_value(f),
            Self::Fork(f) => serde_json::to_value(f),
        }
        .map_err(|e| Error::Serialization(e.to_string()))
    }

    /// The file's bytes, pretty-printed with a trailing newline.
    ///
    /// # Errors
    /// Serialization failure.
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let mut out = serde_json::to_vec_pretty(&self.to_json()?)
            .map_err(|e| Error::Serialization(e.to_string()))?;
        out.push(b'\n');
        Ok(out)
    }

    /// The standard presentation. `file` is how the next commands name the
    /// decisions file (a path for the CLI).
    #[must_use]
    pub fn view(&self, file: &str, surface: Surface) -> MergeView {
        let (kind, title, conflicts, apply) = match self {
            Self::Share(f) => {
                let short = |id: &str| id.chars().take(12).collect::<String>();
                let theirs = |p: &crate::share_merge_decisions::SideProvenance| {
                    vec![format!(
                        "share {}, {}{}",
                        short(&p.theirs.share_id),
                        p.theirs.created_at,
                        if p.theirs.attested { ", attested" } else { "" }
                    )]
                };
                let cards = f
                    .rows
                    .iter()
                    .map(|row| {
                        card(
                            RowParts {
                                id: &row.id,
                                record: &row.record,
                                ours_by: local_by(&row.provenance.ours),
                                theirs_by: theirs(&row.provenance),
                                proposal: row.proposal.as_ref(),
                                decision: row.decision.as_ref(),
                                decided_by: row.decided_by.as_ref(),
                                decided_at: row.decided_at.as_ref(),
                            },
                            &f.labels,
                        )
                    })
                    .collect();
                (
                    "share",
                    format!("share {} -> ROOT", short(&f.incoming_share)),
                    cards,
                    match surface {
                        Surface::Cli => format!(
                            "quipu merge <share-dir> --decisions {file} --reviewer <you> (add --dry-run first)"
                        ),
                        Surface::Mcp => {
                            "quipu_merge_apply with incoming, base, the decisions and reviewer (dry_run: true first)".into()
                        }
                    },
                )
            }
            Self::Fork(f) => {
                let cards = f
                    .rows
                    .iter()
                    .map(|row| {
                        card(
                            RowParts {
                                id: &row.id,
                                record: &row.record,
                                ours_by: local_by(&row.provenance.ours),
                                theirs_by: local_by(&row.provenance.theirs),
                                proposal: row.proposal.as_ref(),
                                decision: row.decision.as_ref(),
                                decided_by: row.decided_by.as_ref(),
                                decided_at: row.decided_at.as_ref(),
                            },
                            &f.labels,
                        )
                    })
                    .collect();
                (
                    "fork",
                    format!("fork '{}' -> ROOT", f.fork),
                    cards,
                    match surface {
                        Surface::Cli => format!(
                            "quipu fork promote {} --decisions {file} --reviewer <you> (add --dry-run first)",
                            f.fork
                        ),
                        Surface::Mcp => format!(
                            "quipu_merge_apply with fork: {:?}, the decisions and reviewer (dry_run: true first)",
                            f.fork
                        ),
                    },
                )
            }
        };
        let conflicts: Vec<ConflictCard> = conflicts;
        let undecided = conflicts.iter().filter(|c| c.decision.is_none()).count();
        let mut next = Vec::new();
        if let Some(first) = conflicts.iter().find(|c| c.decision.is_none()) {
            next.push(match surface {
                Surface::Cli => format!(
                    "quipu merge resolve {file} {} --choose <{}> --by <you>",
                    first.id,
                    first.choices.join("|")
                ),
                Surface::Mcp => format!(
                    "quipu_merge_resolve with the decisions, row: {:?}, choose and by",
                    first.id
                ),
            });
        }
        next.push(apply);
        MergeView {
            schema: VIEW_SCHEMA,
            merge: MergeTarget { kind, title },
            conflicts,
            undecided,
            next,
        }
    }

    /// Record `choice` on row `row`, claimed by `by` at `at`. Writes nothing
    /// to any store; the caller saves the file.
    ///
    /// # Errors
    /// An unknown row, `both` above the slot's `sh:maxCount`, too many
    /// values, or a value that is not one RDF term.
    pub fn resolve(&mut self, row: &str, choice: Choice, by: &str, at: &str) -> Result<()> {
        if by.trim().is_empty() {
            return Err(Error::InvalidValue(
                "say who decided (--by / by): the person who chose, or your agent id".into(),
            ));
        }
        let (record, decision, decided_by, decided_at) = match self {
            Self::Share(f) => {
                let r = f
                    .rows
                    .iter_mut()
                    .find(|r| r.id == row)
                    .ok_or_else(|| unknown_row(row))?;
                (
                    &r.record,
                    &mut r.decision,
                    &mut r.decided_by,
                    &mut r.decided_at,
                )
            }
            Self::Fork(f) => {
                let r = f
                    .rows
                    .iter_mut()
                    .find(|r| r.id == row)
                    .ok_or_else(|| unknown_row(row))?;
                (
                    &r.record,
                    &mut r.decision,
                    &mut r.decided_by,
                    &mut r.decided_at,
                )
            }
        };
        let next = match choice {
            Choice::Defer => None,
            Choice::Side(Side::Both) if !choices(record).iter().any(|c| c == "both") => {
                return Err(Error::InvalidValue(format!(
                    "row {row}: `both` would hold {} values, above sh:maxCount {}",
                    union(&record.ours, &record.theirs).len(),
                    record.max_count
                )));
            }
            Choice::Side(choose) => Some(Decision::Choose { choose }),
            Choice::Values(values) => Some(Decision::Values { values }),
        };
        if next.is_some() {
            decided_values(row, record, next.as_ref())?;
        }
        *decision = next;
        *decided_by = Some(by.to_string());
        *decided_at = Some(at.to_string());
        Ok(())
    }
}

fn unknown_row(row: &str) -> Error {
    Error::InvalidValue(format!("no row {row:?} in these decisions"))
}

#[cfg(test)]
#[path = "merge_reconcile_tests.rs"]
mod tests;
