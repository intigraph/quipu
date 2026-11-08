//! Opt-in evidence from the actual post-state gate, never an admission bypass.
use super::Store;
#[cfg(any(target_os = "linux", feature = "owl"))]
use crate::resolver_contract::ContentDigest;
use crate::resolver_contract::{
    AdmissionCoordinates, ContextCoverage, IncrementalContext, NamedContent, ValidatorFlags,
    ValidatorImplementation, ValidatorManifest,
};
use crate::{Error, Result};

mod snapshot;

/// A single gate execution inside its writer snapshot. Fields cannot be supplied
/// by a caller. Gate success is deliberately distinct from SHACL conformance and
/// from transaction commit: later guards can still roll this transaction back.
#[derive(Debug, Clone)]
pub struct ValidatorObservation {
    manifest: Option<ValidatorManifest>,
    coordinates: Option<AdmissionCoordinates>,
    graph: i64,
    effective_shapes: Option<String>,
    checked_data: Option<String>,
    validator_conforms: Option<bool>,
    gate_error: Option<String>,
    capture_error: Option<String>,
    finished: bool,
}

impl ValidatorObservation {
    /// Actual provisional writer coordinates; not proof that they committed.
    #[must_use]
    pub fn coordinates(&self) -> Option<&AdmissionCoordinates> {
        self.coordinates.as_ref()
    }
    #[must_use]
    pub fn graph(&self) -> i64 {
        self.graph
    }
    /// Exact shapes handed to the executed validator after policy splitting.
    #[must_use]
    pub fn effective_shapes(&self) -> Option<&str> {
        self.effective_shapes.as_deref()
    }
    /// Exact bytes handed to the validator, including duplicates/order.
    #[must_use]
    pub fn checked_data(&self) -> Option<&str> {
        self.checked_data.as_deref()
    }
    #[must_use]
    pub fn validator_conforms(&self) -> Option<bool> {
        self.validator_conforms
    }
    #[must_use]
    pub fn gate_error(&self) -> Option<&str> {
        self.gate_error.as_deref()
    }
    #[must_use]
    pub fn capture_error(&self) -> Option<&str> {
        self.capture_error.as_deref()
    }
    /// The observed DTO may be incomplete. In particular no-shapes, skipped,
    /// exhausted, unexecuted or failed capture never yields Complete coverage.
    /// It confers no admission authority or full staged-graph conformance.
    pub fn manifest(&self) -> Result<&ValidatorManifest> {
        if !self.finished || self.capture_error.is_some() {
            return Err(Error::CannotVerify(
                "validator observation incomplete".into(),
            ));
        }
        self.manifest.as_ref().ok_or_else(|| {
            Error::CannotVerify(
                "actual executable identity unavailable on this build surface".into(),
            )
        })
    }
}

#[derive(Default)]
pub(super) struct ObservationState {
    pub(super) observations: Vec<ValidatorObservation>,
    implementation: Option<ValidatorImplementation>,
}

impl Store {
    /// Capture only the gate executions made by `operation`, including refusals.
    /// No gate execution means an empty vector, never a synthetic Complete.
    /// Scopes restore on unwind; nested scopes also contribute to their parent.
    pub fn observe_validator<T>(
        &mut self,
        operation: impl FnOnce(&mut Self) -> T,
    ) -> (T, Vec<ValidatorObservation>) {
        let previous = self.validator_observation.take();
        let implementation = snapshot::implementation();
        *self.validator_observation.get_mut() = Some(ObservationState {
            observations: Vec::new(),
            implementation,
        });
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| operation(self)));
        let captured = self
            .validator_observation
            .take()
            .expect("observation scope installed")
            .observations;
        *self.validator_observation.get_mut() = previous;
        if let Some(parent) = self.validator_observation.get_mut() {
            parent.observations.extend(captured.iter().cloned());
        }
        match result {
            Ok(value) => (value, captured),
            Err(panic) => std::panic::resume_unwind(panic),
        }
    }

    pub(super) fn begin_validator_observation(
        &mut self,
        graph: i64,
        tx: i64,
        conditional: bool,
    ) -> Option<usize> {
        let implementation = self
            .validator_observation
            .get_mut()
            .as_ref()?
            .implementation
            .clone();
        let mut observation = ValidatorObservation {
            manifest: None,
            coordinates: None,
            graph,
            effective_shapes: None,
            checked_data: None,
            validator_conforms: None,
            gate_error: None,
            capture_error: None,
            finished: false,
        };
        match self.validator_snapshot(graph, tx, conditional, implementation) {
            Ok((manifest, coordinates)) => {
                observation.manifest = manifest;
                observation.coordinates = Some(coordinates);
            }
            Err(error) => observation.capture_error = Some(error.to_string()),
        }
        let state = self.validator_observation.get_mut().as_mut()?;
        let index = state.observations.len();
        state.observations.push(observation);
        Some(index)
    }

    pub(super) fn finish_validator_observation(
        &mut self,
        index: Option<usize>,
        result: &Result<()>,
    ) {
        let Some(observation) = index.and_then(|i| {
            self.validator_observation
                .get_mut()
                .as_mut()?
                .observations
                .get_mut(i)
        }) else {
            return;
        };
        observation.finished = true;
        observation.gate_error = result.as_ref().err().map(ToString::to_string);
        #[cfg(feature = "shacl")]
        if let Err(Error::CannotVerify(message)) = result
            && super::shacl_poststate::context_budget_exhausted(message)
            && let Some(manifest) = &mut observation.manifest
        {
            manifest.incremental.coverage = ContextCoverage::BudgetExhausted;
            manifest
                .implementation
                .options
                .insert("context_exhaustion".into(), message.clone());
        }
    }

    #[cfg(feature = "shacl")]
    pub(super) fn observed_context_exhausted(&mut self, message: &str) {
        if let Some(manifest) = self
            .validator_observation
            .get_mut()
            .as_mut()
            .and_then(|s| s.observations.last_mut())
            .and_then(|o| o.manifest.as_mut())
        {
            manifest.incremental.coverage = ContextCoverage::BudgetExhausted;
            manifest
                .implementation
                .options
                .insert("context_exhaustion".into(), message.into());
        }
    }

    /// Called by the actual debt validator while the gate is still open.
    #[cfg(feature = "shacl")]
    pub(crate) fn observed_debt_call(
        &self,
        phase: &str,
        shapes: &str,
        data: &str,
        result: &Result<crate::shacl::ValidationFeedback>,
    ) {
        let mut state = self.validator_observation.borrow_mut();
        let Some(observation) = state.as_mut().and_then(|s| s.observations.last_mut()) else {
            return;
        };
        if observation.finished {
            return;
        }
        if let Some(manifest) = &mut observation.manifest {
            let value = serde_json::json!({"shapes":shapes,"data":data,
                "conforms":result.as_ref().ok().map(|r| r.conforms),
                "error":result.as_ref().err().map(ToString::to_string)});
            manifest
                .implementation
                .options
                .insert(format!("debt_{phase}"), value.to_string());
            if result.is_err() {
                manifest.incremental.coverage = ContextCoverage::Unknown;
            }
        }
    }

    pub(super) fn observed_loaded_selection(&mut self) {
        if let Some(manifest) = self
            .validator_observation
            .get_mut()
            .as_mut()
            .and_then(|s| s.observations.last_mut())
            .and_then(|o| o.manifest.as_mut())
        {
            manifest.incremental.selected_shapes = manifest.loaded_shapes.clone();
        }
    }

    #[cfg(feature = "shacl")]
    pub(super) fn observed_validator_input(
        &mut self,
        shapes: &str,
        data: &str,
        complete: bool,
        whole: bool,
    ) {
        let Some(observation) = self
            .validator_observation
            .get_mut()
            .as_mut()
            .and_then(|s| s.observations.last_mut())
        else {
            return;
        };
        observation.effective_shapes = Some(shapes.into());
        observation.checked_data = Some(data.into());
        if let Some(manifest) = &mut observation.manifest {
            manifest.incremental.facts = data.lines().map(str::to_owned).collect();
            manifest.incremental.coverage = if complete {
                ContextCoverage::Complete
            } else {
                ContextCoverage::BudgetExhausted
            };
            if whole {
                manifest.incremental.fact_budget = u64::MAX;
            }
            manifest
                .implementation
                .options
                .insert("effective_shapes".into(), shapes.into());
            manifest
                .implementation
                .options
                .insert("checked_data".into(), data.into());
            manifest.implementation.options.insert(
                "context_mode".into(),
                if whole {
                    "whole-new-graph"
                } else {
                    "incremental"
                }
                .into(),
            );
        }
    }

    #[cfg(feature = "shacl")]
    pub(super) fn observed_validator_result(
        &mut self,
        result: &Result<crate::shacl::ValidationFeedback>,
    ) {
        let Some(observation) = self
            .validator_observation
            .get_mut()
            .as_mut()
            .and_then(|s| s.observations.last_mut())
        else {
            return;
        };
        observation.validator_conforms = result.as_ref().ok().map(|v| v.conforms);
        if result.is_err()
            && let Some(manifest) = &mut observation.manifest
        {
            manifest.incremental.coverage = ContextCoverage::Unknown;
        }
    }
}

#[cfg(feature = "owl")]
pub(super) fn cache_source(source: &str) -> ContentDigest {
    snapshot::digest(source.as_bytes())
}

#[cfg(all(test, feature = "shacl", target_os = "linux"))]
mod tests;

#[cfg(all(test, not(feature = "shacl"), target_os = "linux"))]
mod without_shacl {
    use super::*;
    #[test]
    fn unavailable_validator_cannot_claim_complete_context() {
        let mut store = Store::open_in_memory().unwrap();
        let e = store.intern("urn:e").unwrap();
        let a = store.intern("urn:p").unwrap();
        let datum = crate::Datum {
            entity: e,
            attribute: a,
            value: crate::Value::Str("ok".into()),
            valid_from: "2026-01-01T00:00:00Z".into(),
            valid_to: None,
            op: crate::Op::Assert,
        };
        let (result, observations) = store.observe_validator(|s| {
            s.with_write_shapes(Some(""), |s| {
                s.transact(&[datum], "2026-01-01T00:00:00Z", None, None)
            })
        });
        assert!(matches!(result, Err(Error::CannotVerify(_))));
        assert_eq!(observations.len(), 1);
        let m = observations[0].manifest().unwrap();
        assert_eq!(m.incremental.coverage, ContextCoverage::Unknown);
        assert!(!m.implementation.compiled_features.contains("shacl"));
        assert!(
            String::from_utf8_lossy(&m.staged_graph).contains("<urn:e> <urn:p> \"ok\""),
            "actual staged datum omitted"
        );
        assert!(observations[0].gate_error().is_some());
    }
}
