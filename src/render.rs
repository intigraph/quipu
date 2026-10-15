//! Deterministic, bounded graph-to-document plans. No filesystem or network I/O.

#[cfg(not(target_arch = "wasm32"))]
mod diff;
mod plan;
#[cfg(not(target_arch = "wasm32"))]
pub mod publish;
#[cfg(test)]
mod security_tests;
mod spec;
mod template_limits;
#[cfg(test)]
mod tests;

pub use plan::{Markdown, RenderFile, RenderPlan, Renderer, prepare};
pub use spec::{Include, RenderRequest, RenderSpec};

use crate::{Error, Result, Store};
use serde_json::Value;

pub const NS: &str = "urn:quipu:render:";
pub const SPEC_TYPE: &str = "urn:quipu:render:RenderSpec";
pub const MAX_FILES: usize = 2_000;
pub const MAX_BYTES: usize = 16 * 1024 * 1024;

pub(crate) fn invalid(message: impl Into<String>) -> Error {
    Error::InvalidValue(format!("render: {}", message.into()))
}

/// Read-only server seam: return a complete, transaction-bound plan, never
/// interpret a server-side destination or perform a graph write.
pub fn tool_prepare(store: &Store, input: &Value) -> Result<Value> {
    let request: RenderRequest = serde_json::from_value(input.clone())
        .map_err(|e| invalid(format!("invalid request: {e}")))?;
    serde_json::to_value(prepare(store, &request)?).map_err(|e| invalid(e.to_string()))
}

/// Relative portable paths only. Reject traversal, platform aliases and the
/// reserved manifest, rather than normalizing an unsafe input into a safe one.
pub(crate) fn check_path(path: &str) -> Result<()> {
    if path.is_empty() || path.len() > 1_024 || !path.is_ascii() || path.contains(['\\', ':', '\0'])
    {
        return Err(invalid("invalid relative output path"));
    }
    for component in path.split('/') {
        if component.is_empty()
            || component == "."
            || component == ".."
            || component.starts_with(".quipu-render")
            || component.ends_with(['.', ' '])
            || component.chars().any(char::is_control)
        {
            return Err(invalid(format!("unsafe path: {path}")));
        }
    }
    Ok(())
}
