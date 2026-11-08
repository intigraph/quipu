//! Mechanical fork proposals; operator decisions remain explicit.
use super::ForkDecisionRow;
use crate::share_merge_decisions::{LocalFact, Proposal, Side};

pub(super) fn proposal_for(row: &ForkDecisionRow) -> Proposal {
    if crate::merge_review::convergent(&row.record) {
        return Proposal {
            choose: Side::Continue,
            evidence: vec!["both sides agree; preserve ROOT".into()],
        };
    }
    let r = &row.record;
    let newest = |facts: &[LocalFact]| facts.iter().map(|f| f.tx).max();
    let (ours, theirs) = (newest(&row.provenance.ours), newest(&row.provenance.theirs));
    let mut evidence = Vec::new();
    let choose = if r.ours.is_empty() {
        evidence.push(
            "ROOT deleted the slot and the fork replaced it; proposing the replacement".into(),
        );
        Side::Theirs
    } else if r.theirs.is_empty() {
        evidence
            .push("the fork deleted the slot and ROOT replaced it; proposing ROOT's value".into());
        Side::Ours
    } else {
        match (ours, theirs) {
            (Some(o), Some(t)) if o > t => {
                evidence.push(format!("ours is newer: ROOT tx {o} > fork tx {t}"));
                Side::Ours
            }
            (o, t) => {
                evidence.push(format!(
                    "theirs is newer: fork tx {} >= ROOT tx {}",
                    t.map_or("?".into(), |t| t.to_string()),
                    o.map_or("?".into(), |o| o.to_string())
                ));
                Side::Theirs
            }
        }
    };
    evidence.push("mechanical proposal: the operator decides".into());
    Proposal { choose, evidence }
}
