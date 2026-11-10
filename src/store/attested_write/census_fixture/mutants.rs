//! Deliberately wrong fixture policies. Each runs the SAME positive control;
//! a caught assertion failure proves that control distinguishes the naive path.
use std::cell::Cell;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Mutation {
    None,
    TrustClientResult,
    ReservationIsCommit,
    PublishInnerRelease,
    ReceiptIsData,
    OneWakeOneWrite,
    VolatileOnly,
    PropagateObserverError,
}

thread_local! {
    static MODE: Cell<Mutation> = const { Cell::new(Mutation::None) };
}

pub(super) fn mutation() -> Mutation {
    MODE.with(Cell::get)
}

struct Reset;
impl Drop for Reset {
    fn drop(&mut self) {
        MODE.with(|mode| mode.set(Mutation::None));
    }
}

#[test]
fn each_naive_mutant_is_rejected_by_the_same_contract() {
    use super::cases::*;
    let controls: [(Mutation, fn()); 7] = [
        (
            Mutation::TrustClientResult,
            lost_response_recovers_receipt_without_second_write,
        ),
        (
            Mutation::ReservationIsCommit,
            authority_refusal_has_no_receipt_or_committed_bucket,
        ),
        (
            Mutation::PublishInnerRelease,
            tentative_classification_rolls_back_with_outer_write,
        ),
        (
            Mutation::ReceiptIsData,
            receipt_only_settlement_is_not_a_graph_commit,
        ),
        (
            Mutation::OneWakeOneWrite,
            one_coalesced_hint_drains_three_receipts_once,
        ),
        (
            Mutation::VolatileOnly,
            close_reopen_and_lost_ack_preserve_durable_deduplication,
        ),
        (
            Mutation::PropagateObserverError,
            capacity_and_nonfatal_sql_failure_preserve_writer_results_and_report_gaps,
        ),
    ];
    for (mode, control) in controls {
        MODE.with(|active| active.set(mode));
        let _reset = Reset;
        assert!(
            std::panic::catch_unwind(control).is_err(),
            "mutant survived: {mode:?}"
        );
        println!("fixture mutant rejected: {mode:?}");
    }
}
