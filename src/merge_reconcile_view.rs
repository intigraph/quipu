//! The standard presentation's data: one card per conflict (aegis-k7l).

#[allow(clippy::wildcard_imports)]
use super::*;

/// The choices a row offers, in display order.
#[must_use]
pub fn choices(record: &DecisionRecord) -> Vec<String> {
    choices_for(record, true)
}

/// [`choices`], without `value` where the merge cannot take typed values
/// (Git pendant merges).
pub(super) fn choices_for(record: &DecisionRecord, value: bool) -> Vec<String> {
    let mut out = vec!["ours".to_string(), "theirs".into(), "base".into()];
    if union(&record.ours, &record.theirs).len() <= record.max_count {
        out.push("both".into());
    }
    if value {
        out.push("value".into());
    }
    out.push("defer".into());
    out
}

/// The standard presentation of a failed merge.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MergeView {
    pub schema: &'static str,
    pub merge: MergeTarget,
    pub conflicts: Vec<ConflictCard>,
    /// Alias proposals (Git pendant merges only): two new IRIs that may name
    /// one entity. Accept adds `owl:sameAs`; nothing is assumed.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<AliasCard>,
    /// Rows without a decision (deferred rows included).
    pub undecided: usize,
    /// The exact next steps, in order.
    pub next: Vec<String>,
}

/// What is being merged into what.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MergeTarget {
    /// `share` or `fork`.
    pub kind: &'static str,
    /// One line naming both sides, e.g. `fork 'triage' -> ROOT`.
    pub title: String,
}

/// An IRI and its label, when it has one.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Named {
    pub iri: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

/// The rule that made the slot a conflict.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RuleView {
    pub kind: ConflictKind,
    pub max_count: usize,
    pub text: String,
}

/// One side's values and who wrote them.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SideView {
    pub values: Vec<String>,
    pub changed_by: Vec<String>,
}

/// The mechanical proposal, as a suggestion.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Suggestion {
    pub choose: String,
    pub reason: String,
}

/// One alias proposal, as shown to a person.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AliasCard {
    pub id: String,
    pub ours: Named,
    pub theirs: Named,
    pub similarity: f64,
    pub choices: Vec<String>,
    /// `accept`, `reject`, or `None`.
    pub decision: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decided_by: Option<String>,
}

/// One conflicting slot, as shown to a person.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ConflictCard {
    pub id: String,
    pub entity: Named,
    pub field: Named,
    pub rule: RuleView,
    pub base: Vec<String>,
    pub ours: SideView,
    pub theirs: SideView,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggestion: Option<Suggestion>,
    pub choices: Vec<String>,
    /// `ours`, `theirs`, `base`, `both`, `value: ...`, or `None`.
    pub decision: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decided_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decided_at: Option<String>,
}

pub(super) fn side_name(side: Side) -> &'static str {
    match side {
        Side::Ours => "ours",
        Side::Theirs => "theirs",
        Side::Base => "base",
        Side::Both => "both",
    }
}

pub(super) fn decision_text(d: Option<&Decision>) -> Option<String> {
    d.map(|d| match d {
        Decision::Choose { choose } => side_name(*choose).to_string(),
        Decision::Values { values } => format!("value: {}", values.join(", ")),
    })
}

pub(super) fn local_by(facts: &[LocalFact]) -> Vec<String> {
    let mut out: Vec<String> = facts
        .iter()
        .map(|f| {
            format!(
                "{}, {}, tx {}",
                f.actor.as_deref().unwrap_or("unknown"),
                f.valid_from,
                f.tx
            )
        })
        .collect();
    out.dedup();
    out
}

pub(super) fn named(iri: &str, labels: &BTreeMap<String, String>) -> Named {
    let bare = iri
        .strip_prefix('<')
        .and_then(|s| s.strip_suffix('>'))
        .unwrap_or(iri);
    Named {
        iri: iri.to_string(),
        label: labels.get(bare).cloned(),
    }
}

/// A value, with the label of an IRI value appended.
pub(super) fn shown(value: &str, labels: &BTreeMap<String, String>) -> String {
    match named(value, labels).label {
        Some(label) if value.starts_with('<') => format!("{value} ({label})"),
        _ => value.to_string(),
    }
}

pub(super) struct RowParts<'a> {
    pub(super) id: &'a str,
    pub(super) record: &'a DecisionRecord,
    pub(super) ours_by: Vec<String>,
    pub(super) theirs_by: Vec<String>,
    pub(super) proposal: Option<&'a Proposal>,
    pub(super) decision: Option<&'a Decision>,
    pub(super) decided_by: Option<&'a String>,
    pub(super) decided_at: Option<&'a String>,
    pub(super) value: bool,
}

pub(super) fn card(p: RowParts<'_>, labels: &BTreeMap<String, String>) -> ConflictCard {
    let r = p.record;
    let values = |v: &[String]| v.iter().map(|t| shown(t, labels)).collect::<Vec<_>>();
    ConflictCard {
        id: p.id.to_string(),
        entity: named(&r.subject, labels),
        field: named(&r.predicate, labels),
        rule: RuleView {
            kind: kind(r),
            max_count: r.max_count,
            text: rule(r, labels),
        },
        base: values(&r.base),
        ours: SideView {
            values: values(&r.ours),
            changed_by: p.ours_by,
        },
        theirs: SideView {
            values: values(&r.theirs),
            changed_by: p.theirs_by,
        },
        suggestion: p.proposal.map(|pr| Suggestion {
            choose: side_name(pr.choose).into(),
            reason: pr
                .evidence
                .iter()
                .find(|e| !e.starts_with("mechanical proposal"))
                .cloned()
                .unwrap_or_default(),
        }),
        choices: choices_for(r, p.value),
        decision: decision_text(p.decision),
        decided_by: p.decided_by.cloned(),
        decided_at: p.decided_at.cloned(),
    }
}
