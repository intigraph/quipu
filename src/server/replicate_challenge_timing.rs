//! Where a freshness challenge's time goes, from its 202 to its answer frame
//! (aegis-awpw). The follower only sees the total; these stages split it.
//!
//! - `inbox`: 202 until the session producer takes the challenge. Includes the
//!   producer being busy elsewhere: waiting for an in-flight write, or still
//!   sending full catch-up pages.
//! - `admission`: wait for a read permit by a poll that carries a nonce.
//! - `poll`: run time of that poll.
//! - `answer`: 202 until the answering frame is handed to the stream.
//!
//! Stages are a fixed server-owned set; no client or session label, so the
//! series count is constant.
use std::fmt::Write as _;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

const BOUNDS: [f64; 11] = [
    0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Stage {
    Inbox,
    Admission,
    Poll,
    Answer,
}

const STAGES: [Stage; 4] = [Stage::Inbox, Stage::Admission, Stage::Poll, Stage::Answer];

impl Stage {
    const fn name(self) -> &'static str {
        match self {
            Self::Inbox => "inbox",
            Self::Admission => "admission",
            Self::Poll => "poll",
            Self::Answer => "answer",
        }
    }

    const fn index(self) -> usize {
        match self {
            Self::Inbox => 0,
            Self::Admission => 1,
            Self::Poll => 2,
            Self::Answer => 3,
        }
    }
}

struct Histogram {
    // Non-cumulative per bucket; the last slot is above every bound.
    buckets: [AtomicU64; BOUNDS.len() + 1],
    count: AtomicU64,
    sum_micros: AtomicU64,
}

impl Histogram {
    const fn new() -> Self {
        Self {
            buckets: [const { AtomicU64::new(0) }; BOUNDS.len() + 1],
            count: AtomicU64::new(0),
            sum_micros: AtomicU64::new(0),
        }
    }

    fn observe(&self, elapsed: Duration) {
        let seconds = elapsed.as_secs_f64();
        let slot = BOUNDS
            .iter()
            .position(|bound| seconds <= *bound)
            .unwrap_or(BOUNDS.len());
        self.buckets[slot].fetch_add(1, Ordering::Relaxed);
        self.count.fetch_add(1, Ordering::Relaxed);
        let micros = u64::try_from(elapsed.as_micros()).unwrap_or(u64::MAX);
        self.sum_micros.fetch_add(micros, Ordering::Relaxed);
    }
}

static HISTOGRAMS: [Histogram; 4] = [const { Histogram::new() }; 4];

pub(crate) fn observe(stage: Stage, elapsed: Duration) {
    HISTOGRAMS[stage.index()].observe(elapsed);
}

pub(crate) fn render(out: &mut String) {
    out.push_str(
        "# HELP quipu_replicate_challenge_seconds Freshness challenge time by stage, from its 202 to its answer frame.\n# TYPE quipu_replicate_challenge_seconds histogram\n",
    );
    for stage in STAGES {
        let histogram = &HISTOGRAMS[stage.index()];
        let name = stage.name();
        let mut cumulative = 0;
        for (slot, bound) in BOUNDS.iter().enumerate() {
            cumulative += histogram.buckets[slot].load(Ordering::Relaxed);
            let _ = writeln!(
                out,
                "quipu_replicate_challenge_seconds_bucket{{stage=\"{name}\",le=\"{bound}\"}} {cumulative}"
            );
        }
        let count = histogram.count.load(Ordering::Relaxed);
        let _ = writeln!(
            out,
            "quipu_replicate_challenge_seconds_bucket{{stage=\"{name}\",le=\"+Inf\"}} {count}"
        );
        #[allow(clippy::cast_precision_loss)]
        let sum = histogram.sum_micros.load(Ordering::Relaxed) as f64 / 1_000_000.0;
        let _ = writeln!(
            out,
            "quipu_replicate_challenge_seconds_sum{{stage=\"{name}\"}} {sum}"
        );
        let _ = writeln!(
            out,
            "quipu_replicate_challenge_seconds_count{{stage=\"{name}\"}} {count}"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line<'a>(body: &'a str, prefix: &str) -> &'a str {
        body.lines()
            .find(|line| line.starts_with(prefix))
            .unwrap_or_else(|| panic!("missing {prefix} in\n{body}"))
    }

    #[test]
    fn stages_render_cumulative_buckets_with_fixed_labels() {
        let before = {
            let mut body = String::new();
            render(&mut body);
            body
        };
        let count_of = |body: &str, stage: &str| -> u64 {
            line(
                body,
                &format!("quipu_replicate_challenge_seconds_count{{stage=\"{stage}\"}}"),
            )
            .rsplit(' ')
            .next()
            .unwrap()
            .parse()
            .unwrap()
        };
        let le = |body: &str, stage: &str, bound: &str| -> u64 {
            line(
                body,
                &format!(
                    "quipu_replicate_challenge_seconds_bucket{{stage=\"{stage}\",le=\"{bound}\"}}"
                ),
            )
            .rsplit(' ')
            .next()
            .unwrap()
            .parse()
            .unwrap()
        };
        observe(Stage::Answer, Duration::from_millis(3));
        observe(Stage::Answer, Duration::from_millis(700));
        observe(Stage::Answer, Duration::from_secs(9));
        let mut after = String::new();
        render(&mut after);
        assert_eq!(count_of(&after, "answer") - count_of(&before, "answer"), 3);
        // 3 ms lands at le=0.005; 700 ms at le=1; 9 s only in +Inf.
        assert_eq!(
            le(&after, "answer", "0.005") - le(&before, "answer", "0.005"),
            1
        );
        assert_eq!(
            le(&after, "answer", "0.5") - le(&before, "answer", "0.5"),
            1
        );
        assert_eq!(le(&after, "answer", "1") - le(&before, "answer", "1"), 2);
        assert_eq!(le(&after, "answer", "5") - le(&before, "answer", "5"), 2);
        assert_eq!(
            le(&after, "answer", "+Inf") - le(&before, "answer", "+Inf"),
            3
        );
        for stage in ["inbox", "admission", "poll", "answer"] {
            assert!(after.contains(&format!(
                "quipu_replicate_challenge_seconds_sum{{stage=\"{stage}\"}}"
            )));
        }
        assert_eq!(
            after
                .lines()
                .filter(|l| l.starts_with("quipu_replicate_challenge_seconds"))
                .count(),
            4 * (BOUNDS.len() + 1 + 2),
            "series count is fixed"
        );
    }
}
