//! Domain-time selection is separate from deterministic source-event ordering.
use super::{Current, Log, Order, Policy, invalid, reduce};
use crate::Result;
use oxsdatatypes::{DateTime, TimezoneOffset};
use std::str::FromStr;

fn instant(text: &str) -> Result<DateTime> {
    DateTime::from_str(text)
        .ok()
        .filter(|time| time.timezone_offset().is_some())
        .and_then(|time| time.adjust(Some(TimezoneOffset::UTC)))
        .ok_or_else(|| invalid("domain time requires a supported timezone-qualified instant"))
}

/// A half-open domain-time interval. Source recording time remains on immutable
/// observations; these boundaries are normalized UTC domain instants.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DomainInterval {
    pub from: String,
    pub until: Option<String>,
    pub current: Current,
}

impl Log {
    /// Select observations effective by `as_of`, optionally bounded by a full
    /// source-order cut. A transaction number alone is not a global causal cut.
    /// Domain timestamps filter eligibility; they never replace source order as
    /// the latest-policy tie-break. Future corrections can change past results.
    pub fn current_as_of(
        &self,
        fluent: &str,
        policy: Policy,
        as_of: &str,
        through: Option<&Order>,
    ) -> Result<Option<Current>> {
        let at = instant(as_of)?;
        let mut eligible = Vec::new();
        for observation in self.observations(fluent) {
            if through.is_none_or(|cut| &observation.order <= cut)
                && instant(&observation.as_of)? <= at
            {
                eligible.push(observation);
            }
        }
        reduce(&eligible, policy)
    }

    /// Reconstruct domain intervals without deleting raw events or fabricating
    /// a scene graph. Equivalent offset spellings form one boundary. This pure
    /// reader does not prove source authenticity or replace writer admission.
    pub fn domain_intervals(
        &self,
        fluent: &str,
        policy: Policy,
        through: Option<&Order>,
    ) -> Result<Vec<DomainInterval>> {
        let mut boundaries = self
            .observations(fluent)
            .into_iter()
            .filter(|observation| through.is_none_or(|cut| &observation.order <= cut))
            .map(|observation| instant(&observation.as_of))
            .collect::<Result<Vec<_>>>()?;
        // Qualified instants are totally ordered; no unknown-timezone intervals
        // are admitted by instant(). Retain decimal seconds without truncation.
        boundaries.sort_by(|left, right| left.partial_cmp(right).expect("qualified instants"));
        boundaries.dedup();
        let mut intervals: Vec<DomainInterval> = Vec::new();
        for boundary in boundaries {
            let from = boundary.to_string();
            let current = self
                .current_as_of(fluent, policy, &from, through)?
                .ok_or_else(|| invalid("domain boundary without an eligible event"))?;
            // Provenance is part of the projection: equal numbers with different
            // contributing events must not be silently collapsed.
            if intervals
                .last()
                .is_some_and(|prior| prior.current == current)
            {
                continue;
            }
            if let Some(prior) = intervals.last_mut() {
                prior.until = Some(from.clone());
            }
            intervals.push(DomainInterval {
                from,
                until: None,
                current,
            });
        }
        Ok(intervals)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fluent::{Observation, Scalar};
    use std::collections::BTreeSet;
    fn event(id: &str, tx: u64, as_of: &str, value: i64) -> Observation {
        Observation {
            fluent: "urn:fluent".into(),
            order: Order {
                tx,
                writer: "urn:writer".into(),
                event: id.into(),
            },
            as_of: as_of.into(),
            recorded_on: "2026-02-01T00:00:00Z".into(),
            value: Scalar::Integer(value),
        }
    }
    #[test]
    fn late_correction_and_source_cut_are_distinct_from_domain_order() {
        let first = event("urn:one", 1, "2026-01-01T00:00:00Z", 1);
        let second = event("urn:two", 2, "2026-01-02T00:00:00Z", 2);
        let correction = event("urn:fix", 3, "2026-01-01T12:00:00Z", 3);
        let log = Log::try_from(vec![first, second.clone(), correction]).unwrap();
        assert!(
            log.current_as_of("urn:fluent", Policy::Latest, "2025-01-01T00:00:00Z", None)
                .unwrap()
                .is_none()
        );
        let current = log
            .current_as_of("urn:fluent", Policy::Latest, "2026-01-03T00:00:00Z", None)
            .unwrap()
            .unwrap();
        assert_eq!(current.values, BTreeSet::from([Scalar::Integer(3)]));
        let old = log
            .current_as_of(
                "urn:fluent",
                Policy::Latest,
                "2026-01-03T00:00:00Z",
                Some(&second.order),
            )
            .unwrap()
            .unwrap();
        assert_eq!(old.values, BTreeSet::from([Scalar::Integer(2)]));
        let intervals = log
            .domain_intervals("urn:fluent", Policy::Latest, None)
            .unwrap();
        assert_eq!(intervals.len(), 2);
        assert_eq!(intervals[0].until.as_deref(), Some("2026-01-01T12:00:00Z"));
        assert_eq!(intervals[1].current, current);
        assert_eq!(log.observations("urn:fluent").len(), 3);
    }
    #[test]
    fn offsets_fractional_boundaries_and_numeric_contributors_survive() {
        let log = Log::try_from(vec![
            event("urn:a", 1, "2026-01-01T01:00:00.25+01:00", 2),
            event("urn:b", 2, "2026-01-01T00:00:00.25Z", 3),
        ])
        .unwrap();
        let intervals = log
            .domain_intervals("urn:fluent", Policy::Sum, None)
            .unwrap();
        assert_eq!(intervals.len(), 1);
        assert_eq!(intervals[0].from, "2026-01-01T00:00:00.25Z");
        assert_eq!(
            intervals[0].current.values,
            BTreeSet::from([Scalar::Integer(5)])
        );
        assert_eq!(intervals[0].current.produced_by.len(), 2);
        assert!(
            log.current_as_of("urn:fluent", Policy::Sum, "2026-01-01T00:00:00", None)
                .is_err()
        );
    }
}
