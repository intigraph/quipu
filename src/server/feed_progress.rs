//! Scope admission and opt-in empty-range progress frames.
use super::*;

/// Whether a stream may serve this scope: a change feed takes a graph,
/// predicates, subjects and one member condition; an event feed takes types,
/// a group and a graph. No filter is empty or oversized, a term list holds 1
/// to [`feed::MAX_TERM_LIST`] IRIs, and a member condition parses.
pub(crate) fn scope_ok(params: &Params, changes: bool) -> bool {
    let term_list_bad = |list: &Option<String>| {
        list.as_ref()
            .is_some_and(|s| s.len() > 2048 || feed::term_list(s).is_none())
    };
    !(params.progress
        && (!changes
            || params.graph.is_none()
            || params
                .predicate
                .as_deref()
                .and_then(feed::term_list)
                .is_none_or(|v| v.len() != 1)
            || params.subject.is_some()
            || params.member.is_some())
        || changes && (params.types.is_some() || params.group.is_some())
        || !changes
            && (params.predicate.is_some() || params.subject.is_some() || params.member.is_some())
        || params
            .member
            .as_ref()
            .is_some_and(|m| MemberSpec::parse(m).is_err())
        || term_list_bad(&params.predicate)
        || term_list_bad(&params.subject)
        || params
            .graph
            .as_ref()
            .is_some_and(|s| s.is_empty() || s.len() > 2048)
        || params.group.as_ref().is_some_and(|s| s.len() > 2048)
        || params.types.as_ref().is_some_and(|s| s.len() > 2048))
}

impl Cursor {
    pub(super) fn progress_frame(&mut self) -> Option<FeedFrame> {
        if !self.params.progress {
            return None;
        }
        let (hub, _) = self.hub.as_ref()?;
        let (next, observed) = hub.progress(self.scanned, self.progress_source.as_ref()?)?;
        if next < self.offset || observed <= self.progress_observed {
            return None;
        }
        let body = json!({
            "schema": "quipu.change-progress/v1", "source": self.progress_source,
            "scope": {"graphs": [self.params.graph], "predicate": self.params.predicate.as_deref()?.trim()},
            "since": self.offset, "next_tx": next, "watermark_tx": next,
            "records": [], "complete": true, "observed_at": observed,
        });
        self.offset = next;
        self.progress_observed = observed;
        self.wait = true;
        Some(FeedFrame {
            event: "quipu.changes.progress",
            id: Some(next),
            data: body.to_string(),
        })
    }
}
