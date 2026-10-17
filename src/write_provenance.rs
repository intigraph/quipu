//! Who and where a write came from, as a CLASS, never as text (aegis-7zp4rc).
//!
//! Writers declare structured provenance in five headers: `X-Quipu-Agent`,
//! `-Harness`, `-Model`, `-Session` and `-Host`. The values are unbounded, so
//! they are never metric labels. What the metric counts is how COMPLETE the
//! declaration was, per committed write transaction and per normalized client:
//!
//! - `complete`: agent, harness and host, plus session and model when the
//!   harness is an agent harness (`claude` or `codex`);
//! - `absent`: none of the five headers;
//! - `partial`: anything in between.
//!
//! The request middleware classifies the headers once; this module carries the
//! result to the commit, the same way `write_kind` carries the code path. A
//! refused or rolled-back request commits nothing and so counts nothing.

use std::cell::RefCell;
use std::sync::Arc;

/// The provenance headers, in the order the missing-field metric reports them.
pub const FIELDS: [&str; 5] = ["agent", "harness", "host", "session", "model"];

/// Harnesses that run an agent session, for which session and model are
/// required for a declaration to be complete.
const AGENT_HARNESSES: [&str; 2] = ["claude", "codex"];

/// How complete one request's provenance declaration was.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Completeness {
    Complete,
    Partial,
    Absent,
}

impl Completeness {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Complete => "complete",
            Self::Partial => "partial",
            Self::Absent => "absent",
        }
    }
}

/// One request's write provenance: the labels it is counted under, its class,
/// and which REQUIRED fields it lacked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestProvenance {
    /// The normalized client label (bounded by `metrics::normalize_client`).
    pub client: String,
    /// The route template (bounded by the router).
    pub endpoint: String,
    pub completeness: Completeness,
    /// Required fields the request did not declare, as names from `FIELDS`.
    pub missing: Vec<&'static str>,
    /// Bounded caller declarations, never credential evidence.
    pub declared: Declared,
}

/// Retained values supplied by the request seam. Unknown stays null.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Declared {
    pub agent: Option<String>,
    pub harness: Option<String>,
    pub host: Option<String>,
    pub session: Option<String>,
    pub model: Option<String>,
    pub task: Option<String>,
    pub client_version: Option<String>,
}

/// Bound storage at the seam; strip controls and never split UTF-8.
fn normalized(value: Option<&str>) -> Option<String> {
    let text: String = value?
        .chars()
        .filter(|c| !c.is_control())
        .take(128)
        .collect();
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_owned())
}

impl RequestProvenance {
    /// Retain optional task and client version without inventing either.
    #[must_use]
    pub fn with_context(mut self, task: Option<&str>, client_version: Option<&str>) -> Self {
        self.declared.task = normalized(task);
        self.declared.client_version = normalized(client_version);
        self
    }

    /// Classify one request from its five header values, given in `FIELDS`
    /// order: agent, harness, host, session, model.
    #[must_use]
    pub fn classify(client: &str, endpoint: &str, values: [Option<&str>; 5]) -> Self {
        let retained = values.map(normalized);
        let present = retained.each_ref().map(Option::is_some);
        let agent_harness = retained[1]
            .as_deref()
            .is_some_and(|h| AGENT_HARNESSES.contains(&h.to_ascii_lowercase().as_str()));
        let required = if agent_harness { 5 } else { 3 };
        let missing: Vec<&'static str> = FIELDS[..required]
            .iter()
            .zip(present)
            .filter(|(_, p)| !p)
            .map(|(f, _)| *f)
            .collect();
        let completeness = if !present.iter().any(|p| *p) {
            Completeness::Absent
        } else if missing.is_empty() {
            Completeness::Complete
        } else {
            Completeness::Partial
        };
        Self {
            client: client.to_string(),
            endpoint: endpoint.to_string(),
            completeness,
            missing,
            declared: Declared {
                agent: retained[0].clone(),
                harness: retained[1].clone(),
                host: retained[2].clone(),
                session: retained[3].clone(),
                model: retained[4].clone(),
                ..Declared::default()
            },
        }
    }
}

thread_local! {
    static CURRENT: RefCell<Option<Arc<RequestProvenance>>> = const { RefCell::new(None) };
}

/// Run `f` with `provenance` as this thread's request provenance, restoring
/// the previous one. `None` keeps whatever the thread already had.
pub fn scoped<R>(provenance: Option<Arc<RequestProvenance>>, f: impl FnOnce() -> R) -> R {
    let previous = CURRENT.with(|c| {
        let mut slot = c.borrow_mut();
        let previous = slot.clone();
        if provenance.is_some() {
            *slot = provenance;
        }
        previous
    });
    struct Restore(Option<Arc<RequestProvenance>>);
    impl Drop for Restore {
        fn drop(&mut self) {
            let value = self.0.take();
            CURRENT.with(|c| *c.borrow_mut() = value);
        }
    }
    let _restore = Restore(previous);
    f()
}

/// The scoped provenance on this thread, if any (to carry it across a thread
/// hop, and to count it at commit).
#[must_use]
pub fn current() -> Option<Arc<RequestProvenance>> {
    CURRENT.with(|c| c.borrow().clone())
}

#[cfg(test)]
#[path = "write_provenance_tests.rs"]
mod tests;
