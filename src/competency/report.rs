//! Three-state evidence reports; constructing a report never executes a block.
use serde::{Deserialize, Serialize};

/// A failed check and an unavailable check are distinct outcomes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    /// Every applicable check produced positive evidence.
    Pass,
    /// At least one check produced negative evidence.
    Fail,
    /// A required check could not be established.
    Unknown,
}

/// Missing evidence is `None`, never a successful boolean default.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Checks {
    /// The rendered definition parsed as the required read form.
    pub parses: Option<bool>,
    /// Evaluation completed with an interpretable response.
    pub runs: Option<bool>,
    /// Result form, columns, bindings and declared scalar types match.
    pub returns_match: Option<bool>,
    /// The optional declared expectation matches.
    pub expect: Option<bool>,
    /// Every justification target exists in the selected payload or store.
    pub justifies_exist: Option<bool>,
}

impl Checks {
    /// Classify observed checks without treating unavailable evidence as true.
    /// An absent expectation is inapplicable only when the contract has none.
    #[must_use]
    pub fn status(&self, expects: bool) -> Status {
        let required = [
            self.parses,
            self.runs,
            self.returns_match,
            self.justifies_exist,
        ];
        if required.contains(&Some(false)) || self.expect == Some(false) {
            Status::Fail
        } else if required.contains(&None) || (expects && self.expect.is_none()) {
            Status::Unknown
        } else {
            Status::Pass
        }
    }
}

/// One block's evidence; rows are absent when no valid result was obtained.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Block {
    /// Contract-local block identifier.
    pub id: String,
    /// `question` or `command`; commands need their separate staged verifier.
    pub kind: String,
    /// Three-state outcome derived from evidence.
    pub status: Status,
    /// Individual evidence checks.
    pub checks: Checks,
    /// Result cardinality, including a proven zero.
    pub rows: Option<u64>,
    /// Wall time spent on this block.
    pub elapsed_ms: u64,
    /// Bounded diagnostic; never a credential or raw response body.
    pub reason: Option<String>,
}

/// Counts describe actual reported blocks, never an empty-set success.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Summary {
    /// Number of blocks for which a report was emitted.
    pub checked: usize,
    /// Positive block outcomes.
    pub pass: usize,
    /// Negative block outcomes.
    pub fail: usize,
    /// Unestablished block outcomes.
    pub unknown: usize,
}

/// Full verification evidence. The summary is computed from block outcomes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Report {
    /// Stable per-block reports.
    pub blocks: Vec<Block>,
    /// Exact counts over `blocks`.
    pub summary: Summary,
}

impl Report {
    /// Count outcomes without dropping unknown or failed blocks.
    #[must_use]
    pub fn new(blocks: Vec<Block>) -> Self {
        let mut summary = Summary::default();
        for block in &blocks {
            summary.checked += 1;
            match block.status {
                Status::Pass => summary.pass += 1,
                Status::Fail => summary.fail += 1,
                Status::Unknown => summary.unknown += 1,
            }
        }
        Self { blocks, summary }
    }

    /// Exit zero requires at least one checked block and only positive results.
    #[must_use]
    pub fn successful(&self) -> bool {
        !self.blocks.is_empty() && self.blocks.iter().all(|b| b.status == Status::Pass)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn complete() -> Checks {
        Checks {
            parses: Some(true),
            runs: Some(true),
            returns_match: Some(true),
            expect: None,
            justifies_exist: Some(true),
        }
    }

    fn block(status: Status) -> Block {
        Block {
            id: "fixture".into(),
            kind: "question".into(),
            status,
            checks: complete(),
            rows: Some(0),
            elapsed_ms: 1,
            reason: None,
        }
    }

    #[test]
    fn empty_and_unknown_reports_cannot_succeed() {
        assert!(!Report::new(vec![]).successful());
        let report = Report::new(vec![block(Status::Pass), block(Status::Unknown)]);
        assert!(!report.successful());
        assert_eq!(report.summary.checked, 2);
        assert_eq!(report.summary.pass, 1);
        assert_eq!(report.summary.fail, 0);
        assert_eq!(report.summary.unknown, 1);
        assert!(Report::new(vec![block(Status::Pass)]).successful());
    }

    #[test]
    fn every_missing_required_check_is_unknown() {
        for field in 0..4 {
            let mut c = complete();
            match field {
                0 => c.parses = None,
                1 => c.runs = None,
                2 => c.returns_match = None,
                _ => c.justifies_exist = None,
            }
            assert_eq!(c.status(false), Status::Unknown, "field {field}");
        }
        assert_eq!(Checks::default().status(false), Status::Unknown);
    }

    #[test]
    fn missing_expectation_is_not_a_match_when_one_is_declared() {
        let mut c = complete();
        assert_eq!(c.status(false), Status::Pass);
        assert_eq!(c.status(true), Status::Unknown);
        c.expect = Some(false);
        assert_eq!(c.status(true), Status::Fail);
        c.expect = Some(true);
        assert_eq!(c.status(true), Status::Pass);
    }

    #[test]
    fn negative_evidence_is_not_lost_behind_an_unknown() {
        let c = Checks {
            parses: Some(false),
            ..Checks::default()
        };
        assert_eq!(c.status(false), Status::Fail);
        let report = Report::new(vec![block(Status::Fail), block(Status::Unknown)]);
        assert_eq!(report.summary.fail, 1);
        assert_eq!(report.summary.unknown, 1);
        assert!(!report.successful());
    }
}
