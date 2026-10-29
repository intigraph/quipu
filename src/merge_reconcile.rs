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
use crate::git_merge::Decisions as GitDecisions;
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
    /// A Git pendant merge's `<qpack>/decisions.json` (`quipu git-merge`).
    /// Resolved in the repository, because each choice rewrites the pack.
    Git {
        decisions: GitDecisions,
        /// The qpack directory, repository-relative.
        dir: String,
        /// `rdfs:label` per IRI, from the pack's `export.nt`.
        labels: BTreeMap<String, String>,
    },
}

/// Schema of a Git pendant `decisions.json`.
pub const GIT_DECISIONS_SCHEMA: &str = "https://github.com/scbrown/quipu/git-decisions/v1";

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

    /// Read a decisions file from disk. A Git pendant `decisions.json` is
    /// read with its qpack directory (its parent) and the pack's labels.
    ///
    /// # Errors
    /// As [`Self::load`], or the file cannot be read.
    pub fn load_path(path: &std::path::Path) -> Result<Self> {
        let bytes = std::fs::read(path)
            .map_err(|e| Error::Store(format!("read {}: {e}", path.display())))?;
        let value: JsonValue = serde_json::from_slice(&bytes)
            .map_err(|e| Error::InvalidValue(format!("decisions file is not JSON: {e}")))?;
        if value.get("schema").and_then(JsonValue::as_str) != Some(GIT_DECISIONS_SCHEMA) {
            return Self::from_json(value);
        }
        let decisions: GitDecisions = serde_json::from_value(value)
            .map_err(|e| Error::InvalidValue(format!("not a valid Git decisions file: {e}")))?;
        let parent = path.parent().unwrap_or(std::path::Path::new(""));
        let labels = std::fs::read_to_string(parent.join("export.nt"))
            .ok()
            .and_then(|nt| crate::share_merge::parse_graph(&nt, "export.nt").ok())
            .map(|g| {
                // Alias IRIs need labels too; carry them as pseudo-records.
                let pairs: Vec<DecisionRecord> = decisions
                    .aliases
                    .iter()
                    .map(|a| DecisionRecord {
                        subject: a.ours.clone(),
                        predicate: String::new(),
                        max_count: 0,
                        base: vec![a.theirs.clone()],
                        ours: Vec::new(),
                        theirs: Vec::new(),
                    })
                    .collect();
                let records: Vec<&DecisionRecord> =
                    decisions.conflicts.iter().chain(&pairs).collect();
                crate::share_merge_decisions_view::labels(&records, &[&g])
            })
            .unwrap_or_default();
        Ok(Self::Git {
            decisions,
            dir: parent.to_string_lossy().into_owned(),
            labels,
        })
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
            GIT_DECISIONS_SCHEMA => Err(Error::InvalidValue(
                "a Git pendant decisions.json is read from its qpack directory: \
                 `quipu merge show <qpack>/decisions.json`, resolved with \
                 `quipu git-merge resolve <qpack> <key> --choose <choice> --by <who>`"
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
            Self::Git { decisions, .. } => serde_json::to_value(decisions),
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
        let mut aliases: Vec<AliasCard> = Vec::new();
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
                                value: true,
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
                                value: true,
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
            Self::Git {
                decisions,
                dir,
                labels,
            } => {
                let by = |key: &str| decisions.resolved_by.get(key).cloned();
                let cards = decisions
                    .conflicts
                    .iter()
                    .enumerate()
                    .map(|(i, record)| {
                        let key = format!("conflict:{i}");
                        let mut c = card(
                            RowParts {
                                id: &key,
                                record,
                                ours_by: vec!["HEAD".into()],
                                theirs_by: vec!["MERGE_HEAD".into()],
                                proposal: None,
                                decision: None,
                                decided_by: None,
                                decided_at: None,
                                value: false,
                            },
                            labels,
                        );
                        c.decision = decisions.resolutions.get(&key).cloned();
                        c.decided_by = by(&key);
                        c
                    })
                    .collect();
                aliases = decisions
                    .aliases
                    .iter()
                    .enumerate()
                    .map(|(i, a)| {
                        let key = format!("alias:{i}");
                        AliasCard {
                            ours: named(&a.ours, labels),
                            theirs: named(&a.theirs, labels),
                            similarity: a.similarity,
                            choices: vec!["accept".into(), "reject".into(), "defer".into()],
                            decision: decisions.resolutions.get(&key).cloned(),
                            decided_by: by(&key),
                            id: key,
                        }
                    })
                    .collect();
                (
                    "git",
                    format!("qpack '{dir}' (Git merge: MERGE_HEAD -> HEAD)"),
                    cards,
                    format!(
                        "git add {dir} && git commit (CI pendant-check re-runs the merge and verifies the result)"
                    ),
                )
            }
        };
        let conflicts: Vec<ConflictCard> = conflicts;
        let undecided = conflicts.iter().filter(|c| c.decision.is_none()).count()
            + aliases.iter().filter(|a| a.decision.is_none()).count();
        let first = conflicts
            .iter()
            .find(|c| c.decision.is_none())
            .map(|c| (c.id.clone(), c.choices.clone()))
            .or_else(|| {
                aliases
                    .iter()
                    .find(|a| a.decision.is_none())
                    .map(|a| (a.id.clone(), a.choices.clone()))
            });
        let mut next = Vec::new();
        if let Some((id, choices)) = first {
            next.push(match (surface, self) {
                (_, Self::Git { dir, .. }) => format!(
                    "quipu git-merge resolve {dir} {id} --choose <{}> --by <you>",
                    choices.join("|")
                ),
                (Surface::Cli, _) => format!(
                    "quipu merge resolve {file} {id} --choose <{}> --by <you>",
                    choices.join("|")
                ),
                (Surface::Mcp, _) => {
                    format!("quipu_merge_resolve with the decisions, row: {id:?}, choose and by")
                }
            });
        }
        next.push(apply);
        MergeView {
            schema: VIEW_SCHEMA,
            merge: MergeTarget { kind, title },
            conflicts,
            aliases,
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
            Self::Git { dir, .. } => {
                return Err(Error::InvalidValue(format!(
                    "a Git pendant choice rewrites the pack: run `quipu git-merge resolve {dir} {row} --choose <choice> --by {by}`"
                )));
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
