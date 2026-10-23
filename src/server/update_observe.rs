//! Writer store time and phase timing for `/update` (aegis-wlf0).

use std::time::Instant;

use super::{Applied, Attribution, Plan, SharedStore, UpdatePath, apply_locked, update_slice};
use crate::base::AppError;

/// Where one update spent its time under the store lock (aegis-wlf0). Writes
/// recorded no store time at all, so a slower write path was invisible to
/// `quipu_store_held_seconds` and unattributable in the log.
#[derive(Default)]
pub(super) struct UpdateTimes {
    pub(super) dataset: f64,
    pub(super) eval: f64,
    pub(super) gates: f64,
    pub(super) prep: f64,
    pub(super) shapes: f64,
    pub(super) commit: f64,
}

fn ms(seconds: f64) -> f64 {
    (seconds * 1_000_000.0).round() / 1_000.0
}

/// [`apply_update_attributed`], recording its store time for `client`.
pub(super) fn apply_update_observed(
    shared: &SharedStore,
    update: &str,
    force_full: bool,
    attribution: &Attribution,
    client: &str,
) -> Result<Applied, AppError> {
    let plan = if force_full {
        Plan::Full("forced")
    } else {
        update_slice::plan(update)
    };
    let waiting = Instant::now();
    let mut store = shared.lock();
    let wait = waiting.elapsed().as_secs_f64();
    let acquired = Instant::now();
    let mut times = UpdateTimes::default();
    let (result, profile) = store
        .observe_poststate(|store| apply_locked(store, &plan, update, attribution, &mut times));
    // While the guard is still alive, as /query does, so `held` is the
    // lock-holding interval.
    let held = acquired.elapsed().as_secs_f64();
    quipu::metrics::metrics().observe_store_counters(client, "/update", wait, held);
    eprintln!(
        "{}",
        serde_json::json!({
            "event": "update_observation",
            "server_process_id": std::process::id(),
            "client": client,
            "outcome": if result.is_ok() { "ok" } else { "error" },
            "path": result.as_ref().ok().map(|a| match a.path {
                UpdatePath::Sliced => "sliced",
                UpdatePath::Full => "full",
            }),
            "shacl_validate_on_write": store.shacl_config().validate_on_write,
            "store_wait_ms": ms(wait),
            "store_held_ms": ms(held),
            "dataset_ms": ms(times.dataset),
            "eval_ms": ms(times.eval),
            "gates_ms": ms(times.gates),
            "prep_ms": ms(times.prep),
            "explicit_shapes_ms": ms(times.shapes),
            "commit_ms": ms(times.commit),
            "poststate_runs": profile.runs,
            "poststate_load_ms": ms(profile.load.as_secs_f64()),
            "poststate_split_ms": ms(profile.split.as_secs_f64()),
            "poststate_plan_ms": ms(profile.plan.as_secs_f64()),
            "poststate_check_ms": ms(profile.check.as_secs_f64()),
            "conditional_prepare_ms": ms(profile.conditional_prepare.as_secs_f64()),
            "conditional_reject_ms": ms(profile.conditional_reject.as_secs_f64()),
            "conditional_emit_ms": ms(profile.conditional_emit.as_secs_f64()),
        })
    );
    result
}
