//! Progress of superseded-vector retention (aegis-jio5.3).
//!
//! A store upgraded with a backlog drains it over many hourly passes, so the
//! operator needs to see how far it has got and what each delete costs the
//! writer. Counting the remaining backlog would scan every superseded row
//! under the store lock, so it is not exported. Instead, a pass that deleted
//! fewer rows than its cap found nothing more to delete: the drain is complete
//! when `quipu_vector_retention_last_pass_capped` reads 0.
use std::fmt::Write as _;
use std::sync::Mutex;

use super::histogram::Hist;

#[derive(Default)]
struct State {
    pruned_total: u64,
    batches: u64,
    batch_held: Hist,
    passes: u64,
    last_pass_rows: u64,
    last_pass_capped: bool,
    last_pass_end_unix: f64,
}

#[derive(Default)]
pub(super) struct VectorRetentionMetrics {
    state: Mutex<State>,
}

impl VectorRetentionMetrics {
    pub(super) fn observe_batch(&self, deleted: u64, held_seconds: f64) {
        let mut s = self.state.lock().unwrap();
        s.pruned_total += deleted;
        s.batches += 1;
        s.batch_held.observe(held_seconds);
    }

    pub(super) fn observe_pass(&self, rows: u64, capped: bool, end_unix: f64) {
        let mut s = self.state.lock().unwrap();
        s.passes += 1;
        s.last_pass_rows = rows;
        s.last_pass_capped = capped;
        s.last_pass_end_unix = end_unix;
    }

    pub(super) fn render(&self, out: &mut String) {
        let s = self.state.lock().unwrap();
        // Nothing is exported until retention has run, so a store with it
        // disabled (or a replica) does not report a drain it never started.
        if s.passes == 0 && s.batches == 0 {
            return;
        }
        let _ = write!(
            out,
            "# HELP quipu_vector_retention_pruned_total Superseded vectors deleted by retention.\n\
             # TYPE quipu_vector_retention_pruned_total counter\n\
             quipu_vector_retention_pruned_total {}\n\
             # HELP quipu_vector_retention_passes_total Completed hourly retention passes.\n\
             # TYPE quipu_vector_retention_passes_total counter\n\
             quipu_vector_retention_passes_total {}\n\
             # HELP quipu_vector_retention_last_pass_rows Rows the most recent pass deleted.\n\
             # TYPE quipu_vector_retention_last_pass_rows gauge\n\
             quipu_vector_retention_last_pass_rows {}\n\
             # HELP quipu_vector_retention_last_pass_capped 1 if the most recent pass stopped at its \
             per-pass cap (backlog remains), 0 if it found nothing more to delete (drain complete).\n\
             # TYPE quipu_vector_retention_last_pass_capped gauge\n\
             quipu_vector_retention_last_pass_capped {}\n\
             # HELP quipu_vector_retention_last_pass_end_seconds Unix time the most recent pass ended.\n\
             # TYPE quipu_vector_retention_last_pass_end_seconds gauge\n\
             quipu_vector_retention_last_pass_end_seconds {}\n\
             # HELP quipu_vector_retention_batch_held_seconds Store lock held per retention delete \
             batch (excludes waiting for the lock).\n\
             # TYPE quipu_vector_retention_batch_held_seconds histogram\n",
            s.pruned_total,
            s.passes,
            s.last_pass_rows,
            u8::from(s.last_pass_capped),
            s.last_pass_end_unix,
        );
        s.batch_held.render(
            out,
            "quipu_vector_retention_batch_held_seconds",
            "backend=\"sqlite\"",
        );
    }
}

impl super::Metrics {
    /// Record one superseded-vector retention delete batch: rows deleted and
    /// the seconds the store lock was HELD for it (aegis-jio5.3).
    pub fn observe_vector_prune_batch(&self, deleted: u64, held_seconds: f64) {
        self.vector_retention.observe_batch(deleted, held_seconds);
    }

    /// Record the end of one retention pass. `capped` is true when the pass
    /// stopped at its per-pass cap, so a backlog remains.
    pub fn observe_vector_prune_pass(&self, rows: u64, capped: bool, end_unix: f64) {
        self.vector_retention.observe_pass(rows, capped, end_unix);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silent_until_retention_runs() {
        let m = VectorRetentionMetrics::default();
        let mut out = String::new();
        m.render(&mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn reports_progress_and_drain_state() {
        let m = VectorRetentionMetrics::default();
        m.observe_batch(2000, 0.12);
        m.observe_batch(500, 0.30);
        m.observe_pass(2500, false, 1_793_600_000.0);
        let mut out = String::new();
        m.render(&mut out);
        assert!(out.contains("quipu_vector_retention_pruned_total 2500\n"));
        assert!(out.contains("quipu_vector_retention_passes_total 1\n"));
        assert!(out.contains("quipu_vector_retention_last_pass_rows 2500\n"));
        assert!(out.contains("quipu_vector_retention_last_pass_capped 0\n"));
        assert!(out.contains(
            "quipu_vector_retention_batch_held_seconds_bucket{backend=\"sqlite\",le=\"0.25\"} 1\n"
        ));
        assert!(
            out.contains("quipu_vector_retention_batch_held_seconds_count{backend=\"sqlite\"} 2\n")
        );
        m.observe_pass(50_000, true, 1_793_603_600.0);
        let mut out = String::new();
        m.render(&mut out);
        assert!(out.contains("quipu_vector_retention_last_pass_capped 1\n"));
    }
}
