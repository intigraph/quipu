//! Shared, transport-free entry point for local language bindings.
//!
//! This is an incremental surface, not a claim of full native tool parity.
//! Handlers retain native transactions and validation. Caller-supplied actors
//! remain provenance; this API does not manufacture authenticated principals.

use serde::Serialize;
use serde_json::{Value, json};

use crate::{Store, error::Result};

type Handler = fn(&mut Store, &Value) -> Result<Value>;

struct Operation {
    name: &'static str,
    requires_shacl: bool,
    requires_schema: bool,
    handler: Handler,
}

// One registry supplies both dispatch and the advertised operation names.
// Host-dependent tools are deliberately absent until they have real adapters.
const OPERATIONS: &[Operation] = &[
    Operation {
        name: "quipu_query",
        requires_shacl: false,
        requires_schema: false,
        handler: |s, v| crate::tool_query(s, v),
    },
    Operation {
        name: "quipu_export",
        requires_shacl: false,
        requires_schema: false,
        handler: |s, v| crate::tool_export(s, v),
    },
    Operation {
        name: "quipu_entity_history",
        requires_shacl: false,
        requires_schema: false,
        handler: |s, v| crate::tool_entity_history(s, v),
    },
    Operation {
        name: "quipu_knot",
        requires_shacl: true,
        requires_schema: true,
        handler: crate::tool_knot,
    },
    Operation {
        name: "quipu_episode",
        requires_shacl: true,
        requires_schema: true,
        handler: crate::tool_episode,
    },
    Operation {
        name: "quipu_set",
        requires_shacl: true,
        requires_schema: true,
        handler: crate::tool_set,
    },
    Operation {
        name: "quipu_retract",
        requires_shacl: true,
        requires_schema: true,
        handler: crate::tool_retract,
    },
    Operation {
        name: "quipu_shapes",
        requires_shacl: true,
        requires_schema: false,
        handler: |s, v| crate::tool_shapes(s, v),
    },
];

/// A binding error includes a conservative commit disposition, never a retry.
#[derive(Debug, Serialize)]
pub struct BindingError {
    /// Stable binding-level classification; native detail remains in `message`.
    pub code: &'static str,
    /// Requested operation, including unsupported names.
    pub operation: String,
    /// `not_started` for binding refusal; `unknown` after entering a handler.
    pub commit_state: &'static str,
    /// Binding calls never authorize automatic write retries.
    pub retryable: bool,
    /// Native error text or the binding refusal explanation.
    pub message: String,
    /// Native validation, governance, and conflict data when available.
    pub details: Box<Value>,
    #[serde(skip)]
    source: Option<Box<crate::error::Error>>,
}

impl std::fmt::Display for BindingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.operation, self.message)
    }
}

impl std::error::Error for BindingError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source.as_deref().map(|e| e as &dyn std::error::Error)
    }
}

impl BindingError {
    fn new(code: &'static str, operation: &str, state: &'static str, message: String) -> Self {
        Self {
            code,
            operation: operation.into(),
            commit_state: state,
            retryable: false,
            message,
            details: Box::new(Value::Null),
            source: None,
        }
    }

    fn native(operation: &str, error: crate::error::Error) -> Self {
        use crate::error::Error;
        let details = match &error {
            Error::ValidationFailed {
                violations,
                messages,
            } => {
                json!({"kind": "validation_failed", "violations": violations, "messages": messages})
            }
            Error::PolicyDenied(reason) => json!({"kind": "policy_denied", "reason": reason}),
            Error::CannotVerify(reason) => json!({"kind": "cannot_verify", "reason": reason}),
            Error::ConditionalWriteConflict => json!({"kind": "conditional_write_conflict"}),
            Error::Contradiction { entity, attribute } => {
                json!({"kind": "contradiction", "entity": entity, "attribute": attribute})
            }
            _ => Value::Null,
        };
        let mut result = Self::new("engine_error", operation, "unknown", error.to_string());
        result.details = Box::new(details);
        result.source = Some(Box::new(error));
        result
    }
}

/// A local engine owns its store; adapters cannot mutate it around validation.
pub struct Engine {
    store: Store,
    /// Set once this store becomes a replica (the first `replica_*` call).
    /// From then on it is read-only to local callers: only the applier
    /// writes, and it writes what the source already accepted.
    replica: Option<crate::applier::Applier>,
}

impl Engine {
    /// Create a fresh memory store. Shapes must be explicitly adopted through
    /// `quipu_shapes`; writes cannot silently disable a linked validator.
    pub fn open_in_memory() -> Result<Self> {
        #[allow(unused_mut)]
        let mut store = Store::open_in_memory()?;
        #[cfg(feature = "shacl")]
        {
            store.shacl_config_mut().validate_on_write = true;
        }
        Ok(Self {
            store,
            replica: None,
        })
    }

    /// Report this build and actual dispatch availability, not global parity.
    pub fn capabilities(&self) -> Value {
        let schema_adopted = crate::vocabulary::sanctioned(&self.store).map(|v| !v.is_empty());
        json!({
            "contract": "quipu-bindings/v1",
            "version": crate::VERSION,
            "revision": crate::GIT_SHA,
            "git_dirty": env!("QUIPU_GIT_DIRTY") == "true",
            "storage": "memory",
            "features": { "shacl": cfg!(feature = "shacl") },
            "reasoning": "none_registered",
            "shape_catalog": match crate::tool_shapes(&self.store, &json!({"action": "list"})) {
                Ok(catalog) => json!({"state": "available", "catalog": catalog}),
                Err(error) => json!({"state": "unknown", "error": error.to_string()}),
            },
            "schema_state": match &schema_adopted { Ok(true) => "adopted", Ok(false) => "not_adopted", Err(_) => "unknown" },
            "full_parity": false,
            "operations": OPERATIONS.iter().map(|op| json!({
                "name": op.name,
                "available": (!op.requires_shacl || cfg!(feature = "shacl")) && (!op.requires_schema || matches!(schema_adopted, Ok(true))),
                "requires_schema": op.requires_schema,
                "requires_shacl": op.requires_shacl,
            })).collect::<Vec<_>>()
        })
    }

    /// Invoke one exact native tool name. Unsupported and feature-disabled
    /// calls fail before entering any handler or touching the store.
    pub fn invoke(
        &mut self,
        operation: &str,
        input: &Value,
    ) -> std::result::Result<Value, BindingError> {
        let op = OPERATIONS
            .iter()
            .find(|op| op.name == operation)
            .ok_or_else(|| {
                BindingError::new(
                    "unsupported_capability",
                    operation,
                    "not_started",
                    "operation is not available in this binding".into(),
                )
            })?;
        if self.replica.is_some() && writes(operation, input) {
            return Err(BindingError::new(
                "read_only_replica",
                operation,
                "not_started",
                "this store is a replica: only its source's feed writes to it".into(),
            ));
        }
        if op.requires_shacl && !cfg!(feature = "shacl") {
            return Err(BindingError::new(
                "unsupported_capability",
                operation,
                "not_started",
                "operation requires a SHACL-enabled build".into(),
            ));
        }
        if op.requires_schema {
            let vocabulary = crate::vocabulary::sanctioned(&self.store)
                .map_err(|error| BindingError::native(operation, error))?;
            if vocabulary.is_empty() {
                return Err(BindingError::new(
                    "schema_not_adopted", operation, "not_started",
                    "explicitly adopt a non-empty class vocabulary through quipu_shapes before writing".into(),
                ));
            }
        }
        (op.handler)(&mut self.store, input).map_err(|error| {
            // A native error is not evidence of rollback. Preserve UNKNOWN
            // until individual native operations expose a stronger contract.
            BindingError::native(operation, error)
        })
    }

    /// Apply one feed frame through the shared applier, making this store a
    /// read-only replica on first use. Returns the `Outcome` JSON (tagged by
    /// `kind`); a protocol refusal is a returned outcome, never an error.
    ///
    /// The source already validated what it accepted, so the replica applies
    /// it verbatim: local write validation is suspended for the applier's own
    /// writes and restored afterwards. Re-judging source data against local
    /// shapes could refuse it and silently fork the replica.
    ///
    /// # Errors
    /// [`BindingError`] if the local store fails; the frame is not applied.
    pub fn replica_apply_frame(
        &mut self,
        event: &str,
        data: &str,
        now_ms: u64,
    ) -> std::result::Result<Value, BindingError> {
        const OP: &str = "replica_apply_frame";
        if self.replica.is_none() {
            let applier = crate::applier::Applier::open(&self.store)
                .map_err(|error| BindingError::native(OP, error))?;
            self.replica = Some(applier);
        }
        let applier = self.replica.as_mut().expect("opened above");
        #[cfg(feature = "shacl")]
        let validate =
            std::mem::replace(&mut self.store.shacl_config_mut().validate_on_write, false);
        let outcome = applier.apply_frame(&mut self.store, event, data, now_ms);
        #[cfg(feature = "shacl")]
        {
            self.store.shacl_config_mut().validate_on_write = validate;
        }
        let outcome = outcome.map_err(|error| BindingError::native(OP, error))?;
        serde_json::to_value(outcome)
            .map_err(|error| BindingError::new("internal", OP, "unknown", error.to_string()))
    }

    /// The replica cursor `{store_id, log_epoch, source_tx}`, or `null` when
    /// this store is not (yet) a replica.
    #[must_use]
    pub fn replica_progress(&self) -> Value {
        match self
            .replica
            .as_ref()
            .and_then(crate::applier::Applier::progress)
        {
            Some(p) => json!({
                "store_id": p.source.store_id,
                "log_epoch": p.source.log_epoch,
                "source_tx": p.source_tx,
            }),
            None => Value::Null,
        }
    }

    /// `{source_tx, last_frame_age_ms, bounded}`; `bounded` is always false.
    #[must_use]
    pub fn replica_freshness(&self, now_ms: u64) -> Value {
        let freshness = match &self.replica {
            Some(applier) => applier.freshness(now_ms),
            None => crate::applier::Freshness {
                source_tx: None,
                last_frame_age_ms: None,
                bounded: false,
            },
        };
        serde_json::to_value(freshness).unwrap_or(Value::Null)
    }

    /// JSON-text transport preserves native integer lexemes. JS adapters must
    /// not parse this into lossy Numbers without an explicit conversion policy.
    pub fn invoke_json(
        &mut self,
        operation: &str,
        input: &str,
    ) -> std::result::Result<String, BindingError> {
        let input = serde_json::from_str(input).map_err(|error| {
            BindingError::new("invalid_json", operation, "not_started", error.to_string())
        })?;
        let output = self.invoke(operation, &input)?;
        serde_json::to_string(&output).map_err(|error| {
            BindingError::new(
                "serialization_error",
                operation,
                "unknown",
                error.to_string(),
            )
        })
    }
}

/// Whether `operation` with `input` writes to the store. A replica refuses
/// these: its only writer is the applier.
fn writes(operation: &str, input: &Value) -> bool {
    match operation {
        "quipu_knot" | "quipu_episode" | "quipu_set" | "quipu_retract" => true,
        "quipu_shapes" => matches!(
            input.get("action").and_then(Value::as_str),
            Some("load" | "remove")
        ),
        _ => false,
    }
}

#[cfg(test)]
mod tests;
