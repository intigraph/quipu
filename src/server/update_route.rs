//! `/update` routing to an enrolled graph-set owner (aegis-n0wc.5 phase 4 H1).
//!
//! Default OFF: with no `[quipu.graph_sets]` router installed every request is
//! [`Route::Legacy`] without parsing, so today's path is byte-identical. With a
//! router, one decision per request, before any lock and parse-only:
//! - R1: every graph the update names is a member of ONE routed owner, and it
//!   has no default-graph or open reach -> that owner, unchanged request bytes.
//! - R2: no named graph is routed and the reach is a fixed list -> the
//!   ordinary legacy path.
//! - R3: anything else (routed + legacy or default graph, two owners, any
//!   variable graph / `USING` / `WITH` / management reach) -> refused
//!   with 409 naming the graphs. Nothing is evaluated or written, and there is
//!   never a partial commit or a legacy fallback.
use std::collections::BTreeSet;

use oxigraph::model::{GraphName, Quad};

use crate::base::AppError;

/// Where one update goes.
#[cfg(unix)]
pub(crate) enum Route<'a> {
    /// The ordinary legacy writer.
    Legacy,
    /// One routed owner, over exactly these member graphs.
    Owner(
        &'a quipu::store::graph_owner::GraphSetOwner,
        BTreeSet<String>,
    ),
}

/// What `/update` answers: a 409 refusal, or the committed report.
pub(crate) enum Dispatched {
    /// R3: refused before evaluation; nothing was written anywhere.
    Refused(String),
    /// The legacy or routed report.
    Report(serde_json::Value),
}

/// Route and apply one update: the whole `/update` decision, so tests drive
/// exactly the served code with their own router.
#[cfg(unix)]
pub(super) fn dispatch(
    router: Option<&quipu::store::graph_owner::GraphSetRouter>,
    shared: &crate::SharedStore,
    update: &str,
    attribution: &super::Attribution,
    client: &str,
) -> Result<Dispatched, AppError> {
    Ok(match route(router, update) {
        Err(message) => Dispatched::Refused(message),
        Ok(Route::Legacy) => Dispatched::Report(super::apply_update_reported(
            shared,
            update,
            attribution,
            client,
        )?),
        Ok(Route::Owner(owner, members)) => {
            Dispatched::Report(apply_routed(owner, &members, update, attribution, client)?)
        }
    })
}

/// Without unix owners every update is legacy.
#[cfg(not(unix))]
pub(super) fn dispatch(
    _router: Option<&()>,
    shared: &crate::SharedStore,
    update: &str,
    attribution: &super::Attribution,
    client: &str,
) -> Result<Dispatched, AppError> {
    super::apply_update_reported(shared, update, attribution, client).map(Dispatched::Report)
}

#[cfg(unix)]
static ROUTER: std::sync::OnceLock<&'static quipu::store::graph_owner::GraphSetRouter> =
    std::sync::OnceLock::new();

/// Install the process-lifetime router admitted at startup. Once only.
#[cfg(unix)]
#[cfg_attr(not(feature = "ownership-witness-s3"), allow(dead_code))]
pub(crate) fn install(
    router: &'static quipu::store::graph_owner::GraphSetRouter,
) -> Result<(), &'static str> {
    ROUTER
        .set(router)
        .map_err(|_| "a graph-set router is already installed")
}

/// The installed router, if `[quipu.graph_sets]` admitted one.
#[cfg(unix)]
pub(crate) fn router() -> Option<&'static quipu::store::graph_owner::GraphSetRouter> {
    ROUTER.get().copied()
}

/// Decide where `update` (the exact text that will be evaluated) goes.
/// `Err` is the 409 refusal message.
#[cfg(unix)]
pub(crate) fn route<'a>(
    router: Option<&'a quipu::store::graph_owner::GraphSetRouter>,
    update: &str,
) -> Result<Route<'a>, String> {
    use quipu::store::graph_owner::SetTarget;
    let Some(router) = router else {
        return Ok(Route::Legacy);
    };
    // An unparsable update writes nothing on the legacy path either, which
    // reports the same parse error it always has.
    let Ok(Ok(parsed)) =
        quipu::sparql_structure::parse_update(spargebra::SparqlParser::new(), update)
    else {
        return Ok(Route::Legacy);
    };
    let reach = quipu::sparql::update_graphs::update_graphs(&parsed);
    let named: BTreeSet<Option<String>> = reach.named.iter().cloned().map(Some).collect();
    let spans = |why: &str| {
        let list: Vec<&str> = reach.named.iter().map(String::as_str).collect();
        format!(
            "update spans writable files: {why}; graphs [{}]{}{}",
            list.join(", "),
            if reach.default {
                " + default graph"
            } else {
                ""
            },
            if reach.open {
                " + open graph reach"
            } else {
                ""
            },
        )
    };
    // R3 (design list): with graph sets enabled, a reach that is not a fixed
    // list could read or write a member's stale legacy copy. Refuse it even
    // when no routed graph is named.
    if reach.open {
        return Err(spans("open graph reach while graph sets are enabled"));
    }
    if named.is_empty() {
        return Ok(Route::Legacy);
    }
    match router.target_graphs(&named) {
        Ok(SetTarget::Legacy) => Ok(Route::Legacy),
        Ok(SetTarget::Routed(key)) => {
            if reach.default {
                return Err(spans("a routed graph with default-graph reach"));
            }
            let owner = router
                .routed_owner(key)
                .ok_or_else(|| spans("routed owner is not admitted"))?;
            Ok(Route::Owner(owner, reach.named))
        }
        Err(e) => Err(spans(&e.to_string())),
    }
}

/// Every quad the routed update would change is in one of `members`: the
/// owner file holds only its members, but a template could still name ROOT
/// or a graph the walk did not, and that must never commit into this file.
pub(super) fn check_members<'q>(
    changed: impl Iterator<Item = &'q Quad>,
    members: &BTreeSet<String>,
) -> Result<(), AppError> {
    for quad in changed {
        let ok =
            matches!(&quad.graph_name, GraphName::NamedNode(n) if members.contains(n.as_str()));
        if !ok {
            return Err(quipu::Error::PolicyDenied(format!(
                "routed update changes graph {} outside its owner's members",
                quad.graph_name
            ))
            .into());
        }
    }
    Ok(())
}

/// Evaluate and commit a routed update in its owner. The response is the
/// legacy shape plus `owner {store_id, log_epoch, local_tx}`. Like the legacy
/// writer it records store time for `client` and logs one
/// `update_observation` line, marked `"route":"graph_set"` (aegis-n0wc.5 H2a).
#[cfg(unix)]
pub(super) fn apply_routed(
    owner: &quipu::store::graph_owner::GraphSetOwner,
    members: &BTreeSet<String>,
    update: &str,
    attribution: &super::Attribution,
    client: &str,
) -> Result<serde_json::Value, AppError> {
    let plan = super::update_slice::plan(update);
    let mut times = super::update_observe::UpdateTimes::default();
    let mut seen = super::update_observe::RoutedSeen::default();
    let (result, hold) = owner.update_observed(members, |store| {
        seen.store_id = store.store_id().ok();
        seen.shacl_validate_on_write = Some(store.shacl_config().validate_on_write);
        Ok(
            super::apply_locked(store, &plan, update, attribution, &mut times, Some(members))
                .and_then(|applied| {
                    seen.path = Some(applied.path);
                    let mut report = applied.report(store)?;
                    report["owner"] = serde_json::json!({
                        "store_id": store.store_id()?,
                        "log_epoch": store.log_epoch()?,
                        "local_tx": applied.txs.last().map(|(_, tx)| *tx),
                    });
                    Ok(report)
                }),
        )
    });
    let result = result.map_err(AppError::from).and_then(|inner| inner);
    super::update_observe::observe_routed(client, result.is_ok(), &seen, hold, &times);
    let mut report = result?;
    attach_cut(owner, members, &mut report);
    Ok(report)
}

/// How long a receipt may spend capturing its cut. The capture never queues:
/// an occupied fence refuses at once, so this only bounds the capture itself.
#[cfg(unix)]
const CUT_CAPTURE_BUDGET: std::time::Duration = std::time::Duration::from_secs(1);

/// Add `owner.cut` to a committed routed receipt (aegis-n0wc.5 H2b): a strict
/// composition-cut token over the update's members, captured after commit, so
/// each member is at or after this write. If the capture refuses (a fence or
/// reader is occupied) the receipt says `cut: null` and why. The write is
/// already durable, so a failed capture NEVER turns it into an error.
#[cfg(unix)]
fn attach_cut(
    owner: &quipu::store::graph_owner::GraphSetOwner,
    members: &BTreeSet<String>,
    report: &mut serde_json::Value,
) {
    if report["owner"]["local_tx"].is_null() {
        // Nothing committed (e.g. a CAS that matched nothing): no write to cut at.
        return;
    }
    match owner.capture_cut(members, std::time::Instant::now() + CUT_CAPTURE_BUDGET) {
        Ok(cut) => report["owner"]["cut"] = serde_json::json!(cut.to_token()),
        Err(error) => {
            report["owner"]["cut"] = serde_json::Value::Null;
            report["owner"]["cut_unavailable"] = serde_json::json!(error.to_string());
        }
    }
}

#[cfg(all(test, unix))]
#[path = "update_route_tests.rs"]
mod tests;
