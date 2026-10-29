//! MCP tools: `quipu_merge_decisions` / `quipu_merge_apply` (aegis-yavo9c),
//! with forks and `quipu_merge_resolve` added for aegis-k7l.
//!
//! Two tools, not one with a mode, for the reason `align` gives: a client
//! judges a tool by its annotation, and the read that emits (and proposes)
//! decisions must not carry the writer's. The REST routes call these same
//! functions, so the surfaces cannot diverge.
//!
//! Shares arrive INLINE, in the shape `/import` takes, and are verified by
//! hash exactly as a share read from disk. A server-side path is never
//! accepted: that would let a caller name any directory on the server.

use serde_json::{Value as JsonValue, json};

use crate::error::{Error, Result};
use crate::merge_reconcile::{Choice, ReconcileFile, Surface, render_markdown};
use crate::share_merge_decisions::{
    DecisionFile, InlineShare, SharePair, apply_pair, dry_run_pair, emit_pair, emit_pair_scoped,
    propose,
};
use crate::store::Store;

fn share(input: &JsonValue, key: &str) -> Result<InlineShare> {
    let value = input
        .get(key)
        .ok_or_else(|| Error::InvalidValue(format!("missing '{key}' share")))?;
    serde_json::from_value(value.clone())
        .map_err(|e| Error::InvalidValue(format!("'{key}' is not an inline share: {e}")))
}

fn pair(input: &JsonValue) -> Result<SharePair> {
    SharePair::inline(share(input, "incoming")?, share(input, "base")?)
}

/// READ. The conflicts of merging `incoming` into ROOT, as a decisions file;
/// with `propose: true`, each row also carries a mechanical proposal.
///
/// # Errors
/// A missing or unverifiable share, a base that is not the incoming share's
/// parent, or a store read error.
pub fn tool_merge_decisions(store: &Store, input: &JsonValue) -> Result<JsonValue> {
    let propose_rows = input.get("propose").and_then(JsonValue::as_bool) == Some(true);
    let file = if let Some(fork) = fork_name(input)? {
        let mut file = crate::fork_merge_decisions::emit(store, fork)?;
        if propose_rows {
            file = crate::fork_merge_decisions::propose(file);
        }
        ReconcileFile::Fork(file)
    } else {
        ReconcileFile::Share(share_decisions(store, input)?)
    };
    presented(input, &file)
}

fn fork_name(input: &JsonValue) -> Result<Option<&str>> {
    match input.get("fork") {
        None | Some(JsonValue::Null) => Ok(None),
        Some(JsonValue::String(name)) => {
            if input.get("incoming").is_some() || input.get("base").is_some() {
                return Err(Error::InvalidValue(
                    "give either fork or incoming + base, not both".into(),
                ));
            }
            Ok(Some(name))
        }
        Some(_) => Err(Error::InvalidValue("fork must be a fork name".into())),
    }
}

/// The decisions file alone, or with `present: true`
/// `{decisions, presentation, view}`: the standard markdown and its JSON form.
fn presented(input: &JsonValue, file: &ReconcileFile) -> Result<JsonValue> {
    let decisions = file.to_json()?;
    if input.get("present").and_then(JsonValue::as_bool) != Some(true) {
        return Ok(decisions);
    }
    let view = file.view("the decisions", Surface::Mcp);
    Ok(json!({
        "decisions": decisions,
        "presentation": render_markdown(&view),
        "view": view,
    }))
}

/// Pure: record one choice on one row of a decisions file of any merge kind
/// and return the updated file with its presentation. Touches no store.
///
/// # Errors
/// A missing or unknown file, row or choice; a choice the row does not allow.
pub fn tool_merge_resolve(input: &JsonValue) -> Result<JsonValue> {
    let decisions = input
        .get("decisions")
        .cloned()
        .ok_or_else(|| Error::InvalidValue("missing 'decisions'".into()))?;
    let mut file = ReconcileFile::from_json(decisions)?;
    let row = input
        .get("row")
        .and_then(JsonValue::as_str)
        .ok_or_else(|| Error::InvalidValue("missing 'row'".into()))?;
    let by = input
        .get("by")
        .and_then(JsonValue::as_str)
        .ok_or_else(|| Error::InvalidValue("missing 'by': who decided".into()))?;
    let values = match input.get("values") {
        None | Some(JsonValue::Null) => Vec::new(),
        Some(v) => serde_json::from_value::<Vec<String>>(v.clone()).map_err(|e| {
            Error::InvalidValue(format!("values must be a list of N-Triples terms: {e}"))
        })?,
    };
    let choice = Choice::parse(input.get("choose").and_then(JsonValue::as_str), values)?;
    file.resolve(row, choice, by, &crate::time::now_iso())?;
    let mut with = input.clone();
    with["present"] = json!(true);
    presented(&with, &file)
}

fn share_decisions(store: &Store, input: &JsonValue) -> Result<DecisionFile> {
    let scoped = match input.get("slot_scoped") {
        None | Some(JsonValue::Bool(false)) => false,
        Some(JsonValue::Bool(true)) => true,
        Some(_) => return Err(Error::InvalidValue("slot_scoped must be a boolean".into())),
    };
    let mut file = if scoped {
        emit_pair_scoped(store, pair(input)?)?
    } else {
        emit_pair(store, pair(input)?)?
    };
    if input.get("propose").and_then(JsonValue::as_bool) == Some(true) {
        file = propose(file);
    }
    Ok(file)
}

/// WRITE. Finish the merge from a decided file, atomically.
///
/// # Errors
/// As [`crate::share_merge_decisions::apply`]: an undecided row, a stale
/// file, too many values, or an invalid term all refuse and write nothing.
pub fn tool_merge_apply(store: &mut Store, input: &JsonValue) -> Result<JsonValue> {
    let decisions = input
        .get("decisions")
        .ok_or_else(|| Error::InvalidValue("missing 'decisions'".into()))?;
    if let Some(fork) = fork_name(input)? {
        let file: crate::fork_merge_decisions::ForkDecisionFile =
            serde_json::from_value(decisions.clone()).map_err(|e| {
                Error::InvalidValue(format!("'decisions' is not a fork decisions file: {e}"))
            })?;
        let reviewer = input
            .get("reviewer")
            .and_then(JsonValue::as_str)
            .ok_or_else(|| Error::InvalidValue("missing 'reviewer'".into()))?;
        let bytes =
            serde_json::to_vec(decisions).map_err(|e| Error::Serialization(e.to_string()))?;
        let result = crate::fork_merge_decisions::apply(
            store,
            fork,
            &file,
            &bytes,
            reviewer,
            &crate::time::now_iso(),
            input.get("actor").and_then(JsonValue::as_str),
            input.get("dry_run").and_then(JsonValue::as_bool) == Some(true),
        )?;
        return Ok(json!(result));
    }
    let file: DecisionFile = serde_json::from_value(decisions.clone())
        .map_err(|e| Error::InvalidValue(format!("'decisions' is not a decisions file: {e}")))?;
    let reviewer = input
        .get("reviewer")
        .and_then(JsonValue::as_str)
        .ok_or_else(|| Error::InvalidValue("missing 'reviewer'".into()))?;
    let actor = input.get("actor").and_then(JsonValue::as_str);
    // The hash recorded in provenance is of the decisions exactly as received.
    let bytes = serde_json::to_vec(decisions).map_err(|e| Error::Serialization(e.to_string()))?;
    let result = if input.get("dry_run").and_then(JsonValue::as_bool) == Some(true) {
        dry_run_pair(store, pair(input)?, &file, &bytes, reviewer)?
    } else {
        apply_pair(
            store,
            pair(input)?,
            &file,
            &bytes,
            reviewer,
            &crate::time::now_iso(),
            actor,
        )?
    };
    Ok(json!(result))
}
