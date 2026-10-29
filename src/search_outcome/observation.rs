//! One logical call owns terminal accounting, including nested core searches.

use crate::time::Stopwatch;
use ring::rand::SecureRandom;
use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::{Operation, Outcome, Summary, classify};

/// Bounded execution surfaces. A client header alone cannot prove an adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Surface {
    /// Embedded caller with no explicit adapter scope.
    Library,
    /// Local CLI execution.
    CliLocal,
    /// HTTP transport (including unverified external forwarding).
    Http,
    /// Native MCP forwarding through the same HTTP application.
    Mcp,
}

/// Completed evidence for private capture. Default process logs omit query
/// details; fingerprints are correlation, not anonymization.
#[derive(Debug, Clone, Serialize)]
pub struct Event {
    /// Versioned semantic record contract.
    pub schema: &'static str,
    /// Unique logical call identity, nullable only if entropy is unavailable.
    pub logical_call_id: Option<String>,
    /// UTC terminal timestamp.
    pub timestamp: String,
    /// Execution surface established by an adapter scope.
    pub surface: Surface,
    /// Outermost operation; nested operations do not create another event.
    pub operation: Operation,
    /// Input fingerprint, when the core received JSON input.
    pub input_sha256: Option<String>,
    /// Original query text fingerprint, when present.
    pub query_sha256: Option<String>,
    /// HTTP status, absent for local core execution.
    pub status: Option<u16>,
    /// End-to-end scope duration, including adapter waits when scoped there.
    pub duration_ms: u128,
    /// Validated semantic counts and outcome.
    pub summary: Summary,
    /// Private scrubbed request/response evidence; absent before core execution.
    pub details: Option<super::evidence::Details>,
    /// Caller declarations, never authenticated identities or metric labels.
    pub declared_provenance: std::collections::BTreeMap<String, String>,
    /// Explicitly unknown when no caller provenance was supplied.
    pub provenance_knownness: &'static str,
}

#[derive(Default)]
struct Evidence {
    input_captured: bool,
    input_sha256: Option<String>,
    query_sha256: Option<String>,
    summary: Option<Summary>,
    details: Option<super::evidence::Details>,
    transport_status: Option<u16>,
    declared_provenance: std::collections::BTreeMap<String, String>,
}

/// Shared ownership crosses async/blocking boundaries without a second call.
pub struct Call {
    id: Option<String>,
    surface: Surface,
    operation: Operation,
    started: Stopwatch,
    finished: AtomicBool,
    evidence: Mutex<Evidence>,
    terminal: OnceLock<Event>,
}

thread_local! {
    static ACTIVE: RefCell<Option<(Arc<Call>, usize)>> = const { RefCell::new(None) };
}

struct Restore(Option<(Arc<Call>, usize)>);
impl Drop for Restore {
    fn drop(&mut self) {
        ACTIVE.with(|slot| *slot.borrow_mut() = self.0.take());
    }
}

impl Call {
    /// Preserve only documented, bounded provenance fields. Missing fields
    /// remain absent; bearer/authentication headers are never accepted here.
    pub fn declare(&self, fields: &[(&str, Option<&str>)]) {
        let mut evidence = self
            .evidence
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for (key, value) in fields {
            if ![
                "agent", "session", "harness", "host", "model", "task", "client", "purpose",
            ]
            .contains(key)
            {
                continue;
            }
            if let Some(value) = value.filter(|value| {
                !value.is_empty() && value.len() <= 128 && !value.chars().any(char::is_control)
            }) {
                evidence
                    .declared_provenance
                    .entry((*key).into())
                    .or_insert_with(|| super::evidence::scrub_declaration(key, value));
            }
        }
    }
    /// Retain original input even when adapter validation refuses execution.
    pub fn input(&self, input: &Value) {
        self.capture(input, Summary::unavailable(Outcome::Unknown), None);
    }

    /// The native MCP adapter validates its JSON before projecting context.
    pub fn response(&self, input: &Value, response: &Value) {
        self.capture(input, classify(self.operation, response), Some(response));
    }

    /// Native MCP owns final delivery after HTTP serialization/projection.
    #[must_use]
    pub fn is_mcp(&self) -> bool {
        self.surface == Surface::Mcp
    }

    /// Retain an HTTP result without prematurely accounting MCP delivery.
    pub fn transport_finished(&self, status: u16, fallback: Option<Summary>) {
        let mut evidence = self
            .evidence
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        evidence.transport_status = Some(status);
        if evidence.summary.is_none() {
            evidence.summary = fallback;
        }
    }

    /// Complete native MCP delivery; RPC/projection errors override HTTP 200.
    #[must_use]
    pub fn finish_mcp(&self, failure: Option<Outcome>) -> Option<Event> {
        let status = self
            .evidence
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .transport_status;
        self.finish_inner(status, None, failure)
    }
    /// Begin one logical operation. A transport should retain this through its
    /// terminal response, rather than counting a blocking worker's late success.
    #[must_use]
    pub fn new(surface: Surface, operation: Operation) -> Arc<Self> {
        let mut bytes = [0_u8; 16];
        let id = ring::rand::SystemRandom::new()
            .fill(&mut bytes)
            .ok()
            .map(|()| hex::encode(bytes));
        Arc::new(Self {
            id,
            surface,
            operation,
            started: Stopwatch::start(),
            finished: AtomicBool::new(false),
            evidence: Mutex::new(Evidence::default()),
            terminal: OnceLock::new(),
        })
    }

    /// Bind this call to synchronous work and restore a reused worker thread.
    pub fn scope<T>(self: &Arc<Self>, work: impl FnOnce() -> T) -> T {
        let previous = ACTIVE.with(|slot| slot.replace(Some((Arc::clone(self), 0))));
        let _restore = Restore(previous);
        work()
    }

    /// Read terminal evidence without re-emitting or recounting the operation.
    #[must_use]
    pub fn terminal_event(&self) -> Option<&Event> {
        self.terminal.get()
    }

    fn capture(&self, input: &Value, summary: Summary, response: Option<&Value>) {
        let mut evidence = self
            .evidence
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut details =
            super::evidence::Details::capture(input, response, summary.admitted_count.is_some());
        if !evidence.input_captured {
            evidence.input_captured = true;
            evidence.input_sha256 = super::evidence::within_limits(input)
                .then(|| format!("{:x}", Sha256::digest(input.to_string().as_bytes())));
            evidence.query_sha256 = input
                .get("query")
                .and_then(Value::as_str)
                .map(|query| format!("{:x}", Sha256::digest(query.as_bytes())));
        } else if let Some(original) = &evidence.details {
            details.query_evidence.clone_from(&original.query_evidence);
            details.query_redacted = original.query_redacted;
            details.query_bytes = original.query_bytes;
            details
                .requested_options
                .clone_from(&original.requested_options);
        }
        evidence.summary = Some(summary);
        evidence.details = Some(details);
    }

    /// Build at most one terminal event. A timeout/refusal overrides even a
    /// completed core answer; a later worker cannot turn it into a zero/hit.
    #[must_use]
    pub fn finish(&self, status: Option<u16>, fallback: Option<Summary>) -> Option<Event> {
        self.finish_inner(status, fallback, None)
    }

    fn finish_inner(
        &self,
        status: Option<u16>,
        fallback: Option<Summary>,
        failure: Option<Outcome>,
    ) -> Option<Event> {
        if self.finished.swap(true, Ordering::AcqRel) {
            return None;
        }
        let evidence = self
            .evidence
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let summary = match (status, failure) {
            (Some(401 | 403), _) => Summary::unavailable(Outcome::Refused),
            (Some(408 | 504), _) => Summary::unavailable(Outcome::Timeout),
            (Some(499), _) => Summary::unavailable(Outcome::Unknown),
            (Some(code), _) if !(200..300).contains(&code) => Summary::unavailable(Outcome::Error),
            (_, Some(outcome)) => Summary::unavailable(outcome),
            _ => evidence
                .summary
                .or(fallback)
                .unwrap_or_else(|| Summary::unavailable(Outcome::Unknown)),
        };
        let event = Event {
            schema: "quipu-search-outcome/v1",
            logical_call_id: self.id.clone(),
            timestamp: crate::time::now_iso(),
            surface: self.surface,
            operation: self.operation,
            input_sha256: evidence.input_sha256.clone(),
            query_sha256: evidence.query_sha256.clone(),
            status,
            duration_ms: self.started.elapsed_ms(),
            summary,
            details: evidence.details.clone(),
            declared_provenance: evidence.declared_provenance.clone(),
            provenance_knownness: if evidence.declared_provenance.is_empty() {
                "unknown"
            } else if [
                "agent", "session", "harness", "host", "model", "task", "client", "purpose",
            ]
            .iter()
            .all(|key| evidence.declared_provenance.contains_key(*key))
            {
                "declared"
            } else {
                "partial_declared"
            },
        };
        let _ = self.terminal.set(event.clone());
        Some(event)
    }
}

fn error_summary(error: &crate::Error) -> Summary {
    let outcome = match error {
        crate::Error::QueryTimeout { .. }
        | crate::Error::RequestTimeout { .. }
        | crate::Error::WriteAdmissionTimeout { .. } => Outcome::Timeout,
        crate::Error::PolicyDenied(_) => Outcome::Refused,
        _ => Outcome::Error,
    };
    Summary::unavailable(outcome)
}

/// Run a core JSON operation. Nested calls share the outer operation; core
/// returns and errors are unchanged. Explicit transport scopes finish later.
pub fn run(
    operation: Operation,
    input: &Value,
    work: impl FnOnce() -> crate::Result<Value>,
) -> crate::Result<Value> {
    run_recorded(
        operation,
        input,
        work,
        |value| classify(operation, value),
        |value| Some(value),
    )
}

/// Observe a typed core result without cloning or serializing its payload.
pub fn run_with<T>(
    operation: Operation,
    input: &Value,
    work: impl FnOnce() -> crate::Result<T>,
    summarize: impl FnOnce(&T) -> Summary,
) -> crate::Result<T> {
    run_recorded(operation, input, work, summarize, |_| None)
}

fn run_recorded<T>(
    operation: Operation,
    input: &Value,
    work: impl FnOnce() -> crate::Result<T>,
    summarize: impl FnOnce(&T) -> Summary,
    response: impl FnOnce(&T) -> Option<&Value>,
) -> crate::Result<T> {
    let active = ACTIVE.with(|slot| slot.borrow().clone());
    let standalone = active.is_none();
    let (call, depth) = active.unwrap_or_else(|| (Call::new(Surface::Library, operation), 0));
    let previous = ACTIVE.with(|slot| slot.replace(Some((Arc::clone(&call), depth + 1))));
    let _restore = Restore(previous);
    let result = work();
    if depth == 0 {
        let summary = match &result {
            Ok(value) => summarize(value),
            Err(error) => error_summary(error),
        };
        call.capture(input, summary, result.as_ref().ok().and_then(response));
    }
    if standalone && let Some(event) = call.finish(None, None) {
        emit(&event);
    }
    result
}

/// Per-process terminal record count. This is not durable-capture success.
pub fn emitted() -> u64 {
    EMITTED.load(Ordering::Relaxed)
}
static EMITTED: AtomicU64 = AtomicU64::new(0);

/// Emit terminal evidence for the existing process log. Durable capture and
/// semantic counters consume this same event, rather than reclassifying JSON.
pub fn emit(event: &Event) {
    EMITTED.fetch_add(1, Ordering::Relaxed);
    crate::metrics::metrics().observe_search(event);
    super::journal::append(event);
    // Detailed query evidence belongs only in the private journal.
    use std::io::Write as _;
    let _ = writeln!(std::io::stderr().lock(), "{}", log_record(event));
}

fn log_record(event: &Event) -> Value {
    json!({"event":"search_outcome", "logical_call_id":event.logical_call_id,
        "surface":event.surface,"operation":event.operation,"status":event.status,"summary":event.summary})
}

/// Resolve a supported retrieval route into the fixed operation vocabulary.
#[must_use]
pub fn for_route(route: &str) -> Option<Operation> {
    match route {
        "/search" => Some(Operation::Search),
        "/hybrid_search" => Some(Operation::HybridSearch),
        "/search_nodes" | "/search/nodes" => Some(Operation::SearchNodes),
        "/search_facts" => Some(Operation::SearchFacts),
        "/search_query" => Some(Operation::SearchQuery),
        "/query" | "/sparql" => Some(Operation::Query),
        "/ask" => Some(Operation::NamedQuery),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
