//! Portable workflows use the existing guarded REST handlers.
use serde_json::Value;
mod discovery;
mod publication;
mod workflow;

pub(super) fn defs() -> Vec<Value> {
    let mut defs = publication::defs();
    defs.extend(workflow::defs());
    defs.extend(discovery::defs());
    defs
}
