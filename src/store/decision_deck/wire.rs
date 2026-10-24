//! Wire v1 and v2 shapes (decision-deck-seed-decisions.md §1).
//!
//! A v1 request carries `choices` from approve/reject/changes. A v2 request
//! carries curated `labels` and the `actions` it allows, with no `choices`.
//! A request never mixes the two, and a challenge's domain follows its
//! request: v1 challenges serialize to exactly the frozen v1 bytes.

use super::Request;

/// Frozen v1 application domain.
pub(super) const DOMAIN_V1: &str = "quipu-decision-deck-v1";
/// v2: labelled choices, Later and comment.
pub(super) const DOMAIN_V2: &str = "quipu-decision-deck-v2";

const V1_CHOICES: [&str; 3] = ["approve", "reject", "changes"];
const ACTIONS: [&str; 3] = ["choose", "later", "comment"];
const MAX_LABELS: usize = 8;
const MAX_LABEL_BYTES: usize = 200;
const MAX_COMMENT_BYTES: usize = 4000;

fn unique(items: &[String]) -> bool {
    items
        .iter()
        .collect::<std::collections::BTreeSet<_>>()
        .len()
        == items.len()
}

/// Plain text: no control characters at all in a label.
fn plain_label(label: &str) -> bool {
    !label.is_empty() && label.len() <= MAX_LABEL_BYTES && !label.chars().any(char::is_control)
}

/// Plain text: line breaks and tabs only, no other control characters.
fn plain_comment(comment: &str) -> bool {
    comment.len() <= MAX_COMMENT_BYTES
        && !comment
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\n' | '\t'))
}

/// Whether this request uses wire v2.
pub(super) fn is_v2(request: &Request) -> bool {
    !request.labels.is_empty() || !request.actions.is_empty()
}

/// The request's declared choice set is well formed for exactly one wire.
pub(super) fn valid_request(request: &Request) -> bool {
    if is_v2(request) {
        request.choices.is_empty()
            && (1..=MAX_LABELS).contains(&request.labels.len())
            && request.labels.iter().all(|l| plain_label(l))
            && unique(&request.labels)
            && !request.actions.is_empty()
            && request
                .actions
                .iter()
                .all(|a| ACTIONS.contains(&a.as_str()))
            && unique(&request.actions)
    } else {
        !request.choices.is_empty()
            && request.choices.len() <= V1_CHOICES.len()
            && request
                .choices
                .iter()
                .all(|c| V1_CHOICES.contains(&c.as_str()))
            && unique(&request.choices)
    }
}

/// One selection the operator may sign for this request. A v1 request takes
/// no action and no comment. A v2 `choose` names one label exactly; `later`
/// and `comment` name none, and `comment` requires a nonempty comment.
pub(super) fn valid_selection(
    request: &Request,
    action: Option<&str>,
    choice: &str,
    comment: Option<&str>,
) -> bool {
    if !is_v2(request) {
        return action.is_none()
            && comment.is_none()
            && request.choices.iter().any(|c| c == choice);
    }
    let Some(action) = action else {
        return false;
    };
    if !request.actions.iter().any(|a| a == action) || !comment.is_none_or(plain_comment) {
        return false;
    }
    match action {
        "choose" => request.labels.iter().any(|l| l == choice),
        "later" => choice.is_empty(),
        "comment" => choice.is_empty() && comment.is_some_and(|c| !c.trim().is_empty()),
        _ => false,
    }
}
