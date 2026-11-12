//! Bounded pre-write capture with consuming completion, never a commit hook.

use super::*;

/// Evidence supplied by a future authenticated writer adapter, not a capability.
/// No HTTP status or coalesced commit wake may manufacture this declaration.
#[derive(Clone, Copy)]
pub(crate) enum WriteOutcome {
    Committed { transaction: u64 },
    NoDataChange,
    RolledBack,
    Unknown,
}

/// No writer Result is returned or changed by this source-stage observer.
pub(crate) enum StageOutcome<'a> {
    Disabled,
    PlanningRefused,
    Prepared(StagedObservation<'a>),
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum FinalizedObservation {
    /// Transaction identity stays private join evidence, outside aggregate keys.
    DeclaredCommitted {
        transaction: u64,
        recording: RecordOutcome,
    },
    NoDataChange {
        recording: RecordOutcome,
    },
    DiscardedRollback,
    DiscardedUnknown,
}

/// Owns value-free bounded reports, so changing request inputs after capture
/// cannot change what is published. Not Clone: one in-process completion only.
/// This is no durable outbox, exactly-once protocol or restart guarantee.
pub(crate) struct StagedObservation<'a> {
    sink: &'a AggregateSink,
    binding: ProducerBinding,
    reports: Option<Vec<SlotReport>>,
}

impl AggregateSink {
    /// Plan before mutation under the existing admitted capture budget. Disabled
    /// mode skips planning. There is no SQL, writer, queue or configuration here.
    pub(crate) fn try_stage(
        &self,
        mode: ShadowMode,
        binding: ProducerBinding,
        slots: &BTreeMap<SlotKey, ObservationInput<'_>>,
        td: &str,
    ) -> StageOutcome<'_> {
        if mode == ShadowMode::Disabled {
            increment(&self.disabled);
            return StageOutcome::Disabled;
        }
        match observe_batch(slots, td) {
            Ok(reports) => {
                increment(&self.staged);
                StageOutcome::Prepared(StagedObservation {
                    sink: self,
                    binding,
                    reports: Some(reports),
                })
            }
            Err(_) => {
                increment(&self.planning_refused);
                StageOutcome::PlanningRefused
            }
        }
    }
}

impl StagedObservation<'_> {
    /// Consume captured reports only after separate outcome reconciliation.
    /// Unknown/rollback publish no classification. A claimed commit may still
    /// lose observation to a busy/poisoned/full sink; that loss stays explicit.
    /// Accepted classifications never become committed-write counts.
    pub(crate) fn finish(mut self, outcome: WriteOutcome) -> FinalizedObservation {
        let Some(reports) = self.reports.take() else {
            increment(&self.sink.staged_unknown);
            return FinalizedObservation::DiscardedUnknown;
        };
        match outcome {
            WriteOutcome::Committed { transaction }
                if transaction > 0 && i64::try_from(transaction).is_ok() =>
            {
                FinalizedObservation::DeclaredCommitted {
                    transaction,
                    recording: self.sink.try_record(self.binding, &reports),
                }
            }
            WriteOutcome::NoDataChange => FinalizedObservation::NoDataChange {
                recording: self.sink.try_record(self.binding, &reports),
            },
            WriteOutcome::RolledBack => {
                increment(&self.sink.staged_rollback);
                FinalizedObservation::DiscardedRollback
            }
            WriteOutcome::Unknown | WriteOutcome::Committed { .. } => {
                increment(&self.sink.staged_unknown);
                FinalizedObservation::DiscardedUnknown
            }
        }
    }
}

impl Drop for StagedObservation<'_> {
    fn drop(&mut self) {
        if self.reports.is_some() {
            increment(&self.sink.staged_abandoned);
        }
    }
}

#[cfg(test)]
mod tests;
