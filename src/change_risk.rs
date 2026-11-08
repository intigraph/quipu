//! Shared risk vocabulary and scoring for schema and data change plans.
//! Callers must prove the completeness of every required scan.

use serde::Serialize;

/// One input to the risk score.
#[derive(Debug, Serialize)]
pub struct RiskInput {
    /// What was counted.
    pub name: &'static str,
    /// The count.
    pub value: usize,
    /// Points it contributes.
    pub points: u8,
    /// Most points it can contribute.
    pub max: u8,
}

/// The score, its inputs, and the verdict against `threshold`.
#[derive(Debug, Serialize)]
pub struct Risk {
    /// Sum of the inputs' points, capped at 100.
    pub score: u8,
    /// A score above this fails.
    pub threshold: u8,
    /// The judgement.
    pub verdict: Verdict,
    /// Every input, with its points.
    pub inputs: Vec<RiskInput>,
}

/// `pass` 0, `fail` 1, `undetermined` 2: the CLI's exit status.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Verdict {
    /// Determined, at or under the threshold.
    Pass,
    /// Determined, over the threshold.
    Fail,
    /// Could not be computed honestly; never passes.
    Undetermined,
}

impl Verdict {
    /// 0 pass, 1 fail, 2 undetermined.
    #[must_use]
    pub fn exit_code(self) -> i32 {
        match self {
            Self::Pass => 0,
            Self::Fail => 1,
            Self::Undetermined => 2,
        }
    }
}

/// Points per input: `newly_failing_instances` is logarithmic (1 → 20,
/// 10 → 30, 100 → 40, 1000+ → 50); the others are linear and capped.
pub(crate) fn points(name: &str, n: usize) -> (u8, u8) {
    let linear = |per: usize, max: u8| {
        (
            u8::try_from((n * per).min(usize::from(max))).unwrap_or(max),
            max,
        )
    };
    match name {
        "newly_failing_instances" if n == 0 => (0, 50),
        "newly_failing_instances" => {
            let digits = u8::try_from(n.ilog10()).unwrap_or(3).min(3);
            (20 + 10 * digits, 50)
        }
        "stored_queries" => linear(5, 15),
        "named_graphs" => linear(2, 10),
        "snapshots" => linear(5, 10),
        "removed_terms" => linear(5, 10),
        _ => linear(2, 5),
    }
}

/// Sum the inputs' points (capped at 100) and judge against `threshold`.
pub(crate) fn score(inputs: &[(&'static str, usize)], threshold: u8, determined: bool) -> Risk {
    let inputs: Vec<RiskInput> = inputs
        .iter()
        .map(|&(name, value)| {
            let (points, max) = points(name, value);
            RiskInput {
                name,
                value,
                points,
                max,
            }
        })
        .collect();
    let total: u32 = inputs.iter().map(|i| u32::from(i.points)).sum();
    let score = u8::try_from(total.min(100)).unwrap_or(100);
    let verdict = match (determined, score > threshold) {
        (false, _) => Verdict::Undetermined,
        (true, true) => Verdict::Fail,
        (true, false) => Verdict::Pass,
    };
    Risk {
        score,
        threshold,
        verdict,
        inputs,
    }
}
