//! Partial receipts use the shared conflict wire codec, plus an explicit clean row.
use crate::merge_core::{Change, Decision};
use crate::merge_lists::Slot;
use crate::merge_session::Value;
use crate::share_merge_decisions_view::ConflictKind;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ReportRow {
    pub slot: Slot,
    pub kind: Kind,
    pub held: bool,
    pub decision: Option<Decision<Value>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub(crate) enum Kind {
    Conflict(ConflictKind),
    Clean(Clean),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Clean {
    #[serde(rename = "clean")]
    Row,
}

impl Kind {
    pub(super) fn from_change(change: Change, list: bool) -> Self {
        Self::Conflict(match change {
            Change::Convergent => ConflictKind::Convergent,
            Change::MaxcountExceeded | Change::DeleteReplace if list => ConflictKind::ListDivergent,
            Change::MaxcountExceeded => ConflictKind::MaxCountExceeded,
            Change::DeleteReplace => ConflictKind::DeleteReplace,
            _ => return Self::Clean(Clean::Row),
        })
    }
}

pub(super) fn rows(prepared: &crate::merge_session::Prepared) -> Vec<ReportRow> {
    prepared
        .plan
        .slots
        .iter()
        .map(|(slot, p)| ReportRow {
            slot: slot.clone(),
            held: p.held,
            decision: prepared
                .choices
                .iter()
                .find(|choice| choice.slot == *slot)
                .map(|choice| choice.decision.clone()),
            kind: Kind::from_change(
                p.change,
                prepared
                    .request
                    .profile
                    .lists
                    .iter()
                    .any(|b| b.slot == *slot),
            ),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn pending_kinds_use_the_common_canonical_wire_names() {
        for (change, list, expected) in [
            (Change::Unchanged, false, "clean"),
            (Change::Convergent, false, "convergent"),
            (Change::MaxcountExceeded, false, "max_count_exceeded"),
            (Change::DeleteReplace, false, "delete_replace"),
            (Change::MaxcountExceeded, true, "list-divergent"),
            (Change::DeleteReplace, true, "list-divergent"),
        ] {
            let kind = Kind::from_change(change, list);
            assert_eq!(serde_json::to_value(kind).unwrap(), json!(expected));
            assert_eq!(
                serde_json::from_value::<Kind>(json!(expected)).unwrap(),
                kind
            );
        }
    }

    #[test]
    fn old_receipt_aliases_read_but_only_the_common_codec_writes() {
        for (old, canonical) in [
            ("maxcount-exceeded", "max_count_exceeded"),
            ("delete-replace", "delete_replace"),
        ] {
            let kind: Kind = serde_json::from_value(json!(old)).unwrap();
            assert_eq!(serde_json::to_value(kind).unwrap(), json!(canonical));
        }
        assert!(serde_json::from_value::<Kind>(json!("invented-kind")).is_err());
    }
}
