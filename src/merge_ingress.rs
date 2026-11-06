//! Profiled merge ingress shared by CLI, MCP and HTTP. Source-only until
//! qualification. No default activation, new credential or graph grant.
use crate::error::{Error, Result};
use crate::merge_adapters;
use crate::merge_pending;
use crate::merge_session::{Choice, Path, Prepared, Profile, Request};
use crate::store::Store;
use serde::{Deserialize, Serialize};
use serde_json::{Value as Json, json};

pub const SCHEMA: &str = "https://github.com/scbrown/quipu/merge-session/v1";
const MAX_FILE: usize = 8 * 1024 * 1024;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct File {
    pub schema: String,
    pub request: Request,
    pub authored_at: String,
    pub choices: Vec<Choice>,
}

fn invalid(message: &str) -> Error {
    Error::InvalidValue(format!("profiled merge: {message}"))
}
pub(crate) fn profile(value: Option<&Json>) -> Result<Profile> {
    match value {
        None => Ok(Profile::default()),
        Some(value) => serde_json::from_value(value.clone())
            .map_err(|e| invalid(&format!("invalid profile: {e}"))),
    }
}
pub(crate) fn decode(value: &Json) -> Result<File> {
    let bytes = serde_json::to_vec(value).map_err(|e| Error::Serialization(e.to_string()))?;
    if bytes.len() > MAX_FILE {
        return Err(invalid("retained session exceeds 8 MiB"));
    }
    let file: File = serde_json::from_slice(&bytes).map_err(|e| invalid(&e.to_string()))?;
    if file.schema != SCHEMA || file.request.path == Path::Pendant {
        return Err(invalid("not a store merge session"));
    }
    Ok(file)
}

pub(crate) fn id(file: &File) -> Result<String> {
    let mut choices = file.choices.clone();
    choices.sort_by(|a, b| a.slot.cmp(&b.slot));
    Ok(crate::share::sha256(
        &serde_json::to_vec(&(
            "quipu-merge-session/v1",
            &file.request,
            &choices,
            &file.authored_at,
        ))
        .map_err(|e| Error::Serialization(e.to_string()))?,
    ))
}

pub(crate) fn file(
    request: Request,
    authored_at: String,
    choices: Vec<Choice>,
    store: &Store,
    td: &str,
) -> Result<File> {
    let prepared = merge_adapters::prepare_store(store, request, &choices, &authored_at, td)?;
    let mut choices = choices;
    for choice in &prepared.choices {
        if matches!(choice.decision, crate::merge_core::Decision::Continue)
            && !choices.iter().any(|old| old.slot == choice.slot)
        {
            choices.push(choice.clone());
        }
    }
    choices.sort_by(|a, b| a.slot.cmp(&b.slot));
    Ok(File {
        schema: SCHEMA.into(),
        request: prepared.request,
        authored_at,
        choices,
    })
}

fn summary(prepared: &Prepared) -> Json {
    json!({"applied":prepared.plan.applied,"held":prepared.plan.held,"held_units":prepared.held_units,
        "rows":prepared.plan.slots.iter().map(|(slot,p)|json!({"slot":slot,"change":p.change,"held":p.held,
            "decision":prepared.choices.iter().find(|choice|choice.slot==*slot).map(|choice|&choice.decision)})).collect::<Vec<_>>()})
}

pub(crate) fn presented(store: &Store, file: &File, td: &str) -> Result<Json> {
    let plan = merge_adapters::prepare_store(
        store,
        file.request.clone(),
        &file.choices,
        &file.authored_at,
        td,
    )?;
    Ok(json!({"decisions":file,"merge_id":id(file)?,"report":summary(&plan),"writes":false}))
}

/// Normal pre-commit checks re-run at apply. A dry-run is a plan preview;
/// it does not assert that write-only authority/gates have been exercised.
pub(crate) fn apply(
    store: &mut Store,
    file: File,
    ledger: &str,
    reviewer: &str,
    td: &str,
    actor: Option<&str>,
    dry_run: bool,
) -> Result<Json> {
    if reviewer.trim().is_empty() {
        return Err(invalid("reviewer is required"));
    }
    if dry_run {
        let mut preview = presented(store, &file, td)?;
        preview["outcome"] = json!("dry-run");
        preview["write_gate_executed"] = json!(false);
        return Ok(preview);
    }
    crate::transaction_auth::refuse_if_refused()?;
    // Re-applying the same retained file must not re-read no-longer-live ROOT
    // claims before finding its durable, authenticated terminal receipt.
    if let Some(done) = merge_pending::load(store, ledger, &id(&file)?)? {
        return Ok(json!({"outcome":"already-applied","receipt":done}));
    }
    let prepared =
        merge_adapters::prepare_store(store, file.request, &file.choices, &file.authored_at, td)?;
    let receipt =
        merge_adapters::apply_store(store, ledger, prepared, &file.authored_at, td, actor)?;
    Ok(
        json!({"outcome":if receipt.held==0 {"applied"} else {"partial"},"reviewer":reviewer,"receipt":receipt}),
    )
}

/// Local adapters have filesystem authority, and remain unattributed unless
/// called from an authenticated request. Neither API accepts an identity field.
pub fn emit_share(
    store: &Store,
    incoming: &std::path::Path,
    options: &Json,
    td: &str,
) -> Result<Json> {
    let (request, authored) =
        merge_adapters::share_request(store, incoming, profile(options.get("profile"))?)?;
    presented(store, &file(request, authored, Vec::new(), store, td)?, td)
}
pub fn emit_fork(store: &Store, name: &str, options: &Json, td: &str) -> Result<Json> {
    let (request, authored) =
        merge_adapters::fork_request(store, name, profile(options.get("profile"))?)?;
    presented(store, &file(request, authored, Vec::new(), store, td)?, td)
}
pub fn apply_file(
    store: &mut Store,
    value: &Json,
    ledger: &str,
    reviewer: &str,
    td: &str,
    actor: Option<&str>,
    dry_run: bool,
) -> Result<Json> {
    apply(store, decode(value)?, ledger, reviewer, td, actor, dry_run)
}
pub fn resume_file(
    store: &Store,
    ledger: &str,
    parent: &str,
    choices: &Json,
    td: &str,
) -> Result<Json> {
    let parent_receipt =
        merge_pending::load(store, ledger, parent)?.ok_or_else(|| invalid("unknown parent"))?;
    let prepared = merge_adapters::resume(
        store,
        ledger,
        parent,
        &serde_json::from_value::<Vec<Choice>>(choices.clone())
            .map_err(|e| invalid(&e.to_string()))?,
        td,
    )?;
    let file = File {
        schema: SCHEMA.into(),
        request: prepared.request,
        authored_at: parent_receipt.authored_at,
        choices: prepared.reviewed_choices,
    };
    presented(store, &file, td)
}

/// JSON edits only; resolve remains a READ annotation and never opens a store.
pub fn resolve(value: &Json, slot: &Json, decision: &Json) -> Result<Json> {
    let mut file = decode(value)?;
    let slot: crate::merge_lists::Slot =
        serde_json::from_value(slot.clone()).map_err(|e| invalid(&e.to_string()))?;
    let decision = serde_json::from_value(decision.clone()).map_err(|e| invalid(&e.to_string()))?;
    file.choices.retain(|old| old.slot != slot);
    file.choices.push(Choice { slot, decision });
    file.choices.sort_by(|a, b| a.slot.cmp(&b.slot));
    serde_json::to_value(file).map_err(|e| Error::Serialization(e.to_string()))
}

pub(crate) fn require_remote_identity() -> Result<()> {
    match crate::transaction_auth::current_identity() {
        Some(identity)
            if matches!(
                identity.auth_class.as_str(),
                "named_bearer" | "attested_session"
            ) =>
        {
            Ok(())
        }
        _ => Err(Error::PolicyDenied(
            "profiled remote merges require authenticated named or attested identity".into(),
        )),
    }
}
