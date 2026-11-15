//! Fixed-size timing for signed health/identity requests, never authentication.
//!
//! The existing terminal request log owns the request ID and emits one snapshot,
//! including on cancellation. No extra events, labels, headers, bodies, keys,
//! identities or nonces are retained here. Unstarted phases are null; started
//! but unfinished phases report elapsed time with their completion bit unset.

use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;

/// Stable order of `auth_phase_us`. Queue time is separate from mutex wait so
/// blocking-pool saturation cannot be misreported as writer contention.
#[derive(Clone, Copy)]
#[repr(usize)]
pub(super) enum Phase {
    Verify,
    PreDispatch,
    PreWait,
    PreHeld,
    Handler,
    PostDispatch,
    PostWait,
    PostHeld,
}

const COUNT: usize = 8;

#[derive(Clone, Copy, Default)]
struct Timing {
    started: Option<Instant>,
    elapsed: Option<Duration>,
}

#[derive(Clone, Default)]
pub(super) struct Observation(Arc<Mutex<[Timing; COUNT]>>);

impl Observation {
    /// Presence selects observations only; signature verification is unchanged.
    pub(super) fn for_request(method: &str, endpoint: &str, attested: bool) -> Option<Self> {
        (attested
            && matches!(
                (method, endpoint),
                ("GET", "/health") | ("POST", "/identity")
            ))
        .then(Self::default)
    }

    pub(super) fn start(&self, phase: Phase) {
        self.start_at(phase, Instant::now());
    }

    fn start_at(&self, phase: Phase, now: Instant) {
        self.0.lock()[phase as usize].started.get_or_insert(now);
    }

    pub(super) fn finish(&self, phase: Phase) {
        self.finish_at(phase, Instant::now());
    }

    fn finish_at(&self, phase: Phase, now: Instant) {
        let mut phases = self.0.lock();
        let slot = &mut phases[phase as usize];
        if let Some(started) = slot.started {
            slot.elapsed
                .get_or_insert(now.saturating_duration_since(started));
        }
    }

    pub(super) fn transition(&self, from: Phase, to: Phase) {
        let now = Instant::now();
        let mut phases = self.0.lock();
        if let Some(started) = phases[from as usize].started {
            phases[from as usize]
                .elapsed
                .get_or_insert(now.saturating_duration_since(started));
        }
        phases[to as usize].started.get_or_insert(now);
    }

    pub(super) fn annotate(&self, record: &mut serde_json::Value) {
        self.annotate_at(record, Instant::now());
    }

    fn annotate_at(&self, record: &mut serde_json::Value, now: Instant) {
        let phases = self.0.lock();
        let mut started = 0_u16;
        let mut complete = 0_u16;
        let mut micros = [None; COUNT];
        for (index, slot) in phases.iter().enumerate() {
            if let Some(begin) = slot.started {
                started |= 1 << index;
                if slot.elapsed.is_some() {
                    complete |= 1 << index;
                }
                let duration = slot
                    .elapsed
                    .unwrap_or_else(|| now.saturating_duration_since(begin));
                micros[index] = Some(u64::try_from(duration.as_micros()).unwrap_or(u64::MAX));
            }
        }
        record["auth_phase_us"] = serde_json::json!(micros);
        record["auth_phase_started"] = serde_json::json!(started);
        record["auth_phase_complete"] = serde_json::json!(complete);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_is_bounded_and_is_not_an_authentication_result() {
        for (method, path) in [("GET", "/health"), ("POST", "/identity")] {
            assert!(Observation::for_request(method, path, true).is_some());
            assert!(Observation::for_request(method, path, false).is_none());
        }
        assert!(Observation::for_request("POST", "/query", true).is_none());
        assert!(Observation::for_request("GET", "/identity", true).is_none());
        assert!(std::mem::size_of::<[Timing; COUNT]>() <= 512);
    }

    #[test]
    fn cancelled_wait_is_partial_and_unstarted_work_is_unknown() {
        let trace = Observation::default();
        let base = Instant::now();
        trace.start_at(Phase::Verify, base);
        trace.finish_at(Phase::Verify, base + Duration::from_micros(3));
        trace.start_at(Phase::PreWait, base + Duration::from_micros(3));
        let mut record = serde_json::json!({"request_id": 7, "status": 499});
        trace.annotate_at(&mut record, base + Duration::from_micros(15));
        assert_eq!(
            record["auth_phase_us"],
            serde_json::json!([3, null, 12, null, null, null, null, null])
        );
        assert_eq!(record["auth_phase_started"], 5);
        assert_eq!(record["auth_phase_complete"], 1);
        assert_eq!(record["request_id"], 7);
        assert_eq!(record["status"], 499);
    }

    #[test]
    fn completion_is_stable_and_contains_only_fixed_numeric_fields() {
        let trace = Observation::default();
        let base = Instant::now();
        trace.start_at(Phase::Handler, base);
        trace.finish_at(Phase::Handler, base + Duration::from_micros(0));
        trace.finish_at(Phase::Handler, base + Duration::from_secs(1));
        let mut record = serde_json::json!({});
        trace.annotate_at(&mut record, base + Duration::from_secs(2));
        assert_eq!(record.as_object().unwrap().len(), 3);
        assert_eq!(record["auth_phase_us"][4], 0);
        assert_eq!(record["auth_phase_complete"], 16);
        assert!(
            record["auth_phase_us"]
                .as_array()
                .unwrap()
                .iter()
                .all(|v| v.is_null() || v.is_u64())
        );
        assert!(serde_json::to_vec(&record).unwrap().len() < 256);
    }

    #[test]
    fn worst_case_terminal_record_has_a_fixed_byte_bound() {
        let trace = Observation::default();
        for timing in trace.0.lock().iter_mut() {
            timing.started = Some(Instant::now());
            timing.elapsed = Some(Duration::MAX);
        }
        let mut record = serde_json::json!({});
        trace.annotate(&mut record);
        assert_eq!(record["auth_phase_started"], 255);
        assert_eq!(record["auth_phase_complete"], 255);
        assert!(
            record["auth_phase_us"]
                .as_array()
                .unwrap()
                .iter()
                .all(|v| v.as_u64() == Some(u64::MAX))
        );
        assert!(serde_json::to_vec(&record).unwrap().len() < 256);
    }
}
