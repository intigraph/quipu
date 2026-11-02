//! MCP tool implementations for schema evolution proposals.

use serde_json::Value as JsonValue;

use crate::error::{Error, Result};
use crate::store::Store;

/// MCP tool: `quipu_propose_schema_change` -- Submit a schema evolution proposal.
pub fn tool_propose_schema_change(store: &Store, input: &JsonValue) -> Result<JsonValue> {
    let kind_str = input
        .get("kind")
        .and_then(|v| v.as_str())
        .ok_or_else(|| Error::InvalidValue("missing 'kind' parameter".into()))?;
    let kind = crate::proposal::ProposalKind::from_json(kind_str)?;
    let target = input
        .get("target")
        .and_then(|v| v.as_str())
        .ok_or_else(|| Error::InvalidValue("missing 'target' parameter".into()))?;
    let diff = input
        .get("diff")
        .and_then(|v| v.as_str())
        .ok_or_else(|| Error::InvalidValue("missing 'diff' parameter".into()))?;
    let rationale = input.get("rationale").and_then(|v| v.as_str());
    let proposer = input
        .get("proposer")
        .and_then(|v| v.as_str())
        .ok_or_else(|| Error::InvalidValue("missing 'proposer' parameter".into()))?;
    let trigger_ref = input.get("trigger_ref").and_then(|v| v.as_str());
    // Checked in full before the proposal exists, so a bad program stores nothing.
    let migration = input
        .get("migration")
        .filter(|v| !v.is_null())
        .map(crate::proposal_migration::parse_program)
        .transpose()?;
    let now = crate::time::now_iso();
    let timestamp = input
        .get("timestamp")
        .and_then(|v| v.as_str())
        .unwrap_or(&now);

    let id = store.insert_proposal(&crate::proposal::NewProposal {
        kind: &kind,
        target,
        diff,
        rationale,
        proposer,
        trigger_ref,
        created_at: timestamp,
    })?;
    if let Some(program) = &migration {
        store.set_proposal_migration(id, program)?;
    }

    Ok(serde_json::json!({
        "proposal_id": id,
        "status": "pending",
        "migration_ops": migration.as_ref().map_or(0, Vec::len),
    }))
}

/// MCP tool: `quipu_list_proposals` -- List schema evolution proposals.
pub fn tool_list_proposals(store: &Store, input: &JsonValue) -> Result<JsonValue> {
    let status = input
        .get("status")
        .and_then(|v| v.as_str())
        .map(crate::proposal::ProposalStatus::from_json)
        .transpose()?;

    let with_plan = input
        .get("plan")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    let threshold = risk_threshold(input)?;
    let proposals = store.list_proposals(status.as_ref())?;
    let items: Vec<JsonValue> = proposals
        .iter()
        .map(|p| {
            // aegis-493: only a pending proposal has a blast radius to preview.
            let plan = (with_plan && p.status == crate::proposal::ProposalStatus::Pending)
                .then(|| crate::proposal_plan::plan(store, p.id, threshold))
                .transpose()?;
            Ok(serde_json::json!({
                "id": p.id,
                "kind": p.kind,
                "target": p.target,
                "diff": p.diff,
                "rationale": p.rationale,
                "proposer": p.proposer,
                "trigger_ref": p.trigger_ref,
                "status": p.status,
                "decided_by": p.decided_by,
                "decided_at": p.decided_at,
                "decision_note": p.decision_note,
                "created_at": p.created_at,
                "migration": store.get_proposal_migration(p.id)?,
                "plan": plan,
            }))
        })
        .collect::<Result<_>>()?;

    Ok(serde_json::json!({
        "proposals": items,
        "count": items.len()
    }))
}

/// MCP tool: `quipu_accept_proposal` -- Accept a pending schema proposal,
/// running its migration program if it carries one. With `dry_run: true` it
/// accepts nothing and reports what acceptance would do (aegis-esd): which
/// existing facts the new shapes would refuse, what the migration would
/// change, and whether the acceptance would go through.
pub fn tool_accept_proposal(store: &mut Store, input: &JsonValue) -> Result<JsonValue> {
    let id = input
        .get("id")
        .and_then(serde_json::Value::as_i64)
        .ok_or_else(|| Error::InvalidValue("missing 'id' parameter".into()))?;
    if input
        .get("dry_run")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
    {
        let limit = input
            .get("limit")
            .and_then(serde_json::Value::as_u64)
            .map_or(50, |n| usize::try_from(n).unwrap_or(usize::MAX).min(1000));
        return crate::proposal_migration::dry_run(store, id, limit);
    }
    let decided_by = input
        .get("decided_by")
        .and_then(|v| v.as_str())
        .ok_or_else(|| Error::InvalidValue("missing 'decided_by' parameter".into()))?;
    let now = crate::time::now_iso();
    let timestamp = input
        .get("timestamp")
        .and_then(|v| v.as_str())
        .unwrap_or(&now);
    let note = input.get("note").and_then(|v| v.as_str());

    // aegis-493: plan the blast radius first; refuse a failing or undetermined
    // plan unless the caller accepts the risk with a reason.
    let plan = crate::proposal_plan::plan_for_accept(store, id, risk_threshold(input)?)?;
    let accept_risk = input
        .get("accept_risk")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    let reason = input.get("risk_reason").and_then(|v| v.as_str());
    if accept_risk && reason.is_none_or(|r| r.trim().is_empty()) {
        return Err(Error::InvalidValue(
            "accept_risk needs a non-empty 'risk_reason'".into(),
        ));
    }
    let note = match &plan {
        Some(plan) => crate::proposal_plan::gate(plan, reason.filter(|_| accept_risk), note)?,
        None => note.map(str::to_owned),
    };

    let migrates = store.get_proposal_migration(id)?.is_some();
    let (proposal, migration_tx) =
        store.accept_proposal_with_migration(id, decided_by, timestamp, note.as_deref())?;

    Ok(serde_json::json!({
        "proposal_id": proposal.id,
        "status": "accepted",
        "target": proposal.target,
        "kind": proposal.kind,
        "migration_source": migrates.then(|| crate::proposal_migration::migration_source(id)),
        "migration_tx": migration_tx,
        "plan": plan,
    }))
}

/// `risk_threshold` (0-100), else the deployment default (aegis-493).
fn risk_threshold(input: &JsonValue) -> Result<u8> {
    match input.get("risk_threshold").filter(|v| !v.is_null()) {
        None => Ok(crate::proposal_plan::default_threshold()),
        Some(v) => v
            .as_u64()
            .filter(|t| *t <= 100)
            .and_then(|t| u8::try_from(t).ok())
            .ok_or_else(|| Error::InvalidValue("'risk_threshold' must be an integer 0-100".into())),
    }
}

/// MCP tool: `quipu_reject_proposal` -- Reject a pending schema proposal.
pub fn tool_reject_proposal(store: &Store, input: &JsonValue) -> Result<JsonValue> {
    let id = input
        .get("id")
        .and_then(serde_json::Value::as_i64)
        .ok_or_else(|| Error::InvalidValue("missing 'id' parameter".into()))?;
    let decided_by = input
        .get("decided_by")
        .and_then(|v| v.as_str())
        .ok_or_else(|| Error::InvalidValue("missing 'decided_by' parameter".into()))?;
    let now = crate::time::now_iso();
    let timestamp = input
        .get("timestamp")
        .and_then(|v| v.as_str())
        .unwrap_or(&now);
    let note = input
        .get("note")
        .and_then(|v| v.as_str())
        .ok_or_else(|| Error::InvalidValue("missing 'note' (rejection reason) parameter".into()))?;

    let proposal = store.reject_proposal(id, decided_by, timestamp, note)?;

    Ok(serde_json::json!({
        "proposal_id": proposal.id,
        "status": "rejected",
        "target": proposal.target,
    }))
}
