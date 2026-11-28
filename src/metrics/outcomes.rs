//! Fixed terminal HTTP outcome vocabulary. Never use caller-controlled labels.

pub(super) const LABELS: [&str; 6] = [
    "ok",
    "client_error",
    "timeout",
    "client_closed",
    "unavailable",
    "server_error",
];

/// Include protocol upgrades and redirects as non-error completions. Invalid
/// codes remain counted as server errors rather than disappearing from totals.
pub(super) fn classify(status: u16) -> usize {
    match status {
        100..=399 => 0,
        408 => 2,
        499 => 3,
        400..=499 => 1,
        503 => 4,
        _ => 5,
    }
}

pub(super) fn render(
    out: &mut String,
    clients: &std::collections::BTreeMap<super::ClientTaskEndpoint, super::RequestObservation>,
) {
    use super::esc;
    use std::fmt::Write as _;
    out.push_str(
            "# HELP quipu_http_client_requests_total Requests completed, by normalised caller, task, route template, and bounded outcome.\n\
             # TYPE quipu_http_client_requests_total counter\n",
        );
    for ((client, task, ep), (counts, _secs)) in clients {
        for (outcome, n) in LABELS.iter().zip(counts) {
            let _ = writeln!(
                out,
                "quipu_http_client_requests_total{{client=\"{}\",task=\"{}\",endpoint=\"{}\",outcome=\"{}\"}} {n}",
                esc(client),
                esc(task),
                esc(ep),
                outcome
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn membership(status: u16) -> [bool; 6] {
        [
            (100..400).contains(&status),
            (400..500).contains(&status) && status != 408 && status != 499,
            status == 408,
            status == 499,
            status == 503,
            (500..600).contains(&status) && status != 503,
        ]
    }

    #[test]
    fn every_http_status_belongs_to_exactly_one_class() {
        for status in 100..600 {
            let classes = membership(status);
            assert_eq!(classes.iter().filter(|&&yes| yes).count(), 1);
            assert!(classes[classify(status)], "status {status}");
        }
    }

    #[test]
    fn observed_statuses_and_invalid_boundaries_are_named() {
        for (status, expected) in [
            (101, "ok"),
            (200, "ok"),
            (202, "ok"),
            (302, "ok"),
            (400, "client_error"),
            (401, "client_error"),
            (429, "client_error"),
            (408, "timeout"),
            (499, "client_closed"),
            (503, "unavailable"),
            (500, "server_error"),
            (0, "server_error"),
            (99, "server_error"),
            (600, "server_error"),
            (u16::MAX, "server_error"),
        ] {
            assert_eq!(LABELS[classify(status)], expected);
        }
    }

    proptest! {
        #[test]
        fn classifier_is_total_and_partitions_valid_codes(status in any::<u16>()) {
            let index = classify(status);
            prop_assert!(index < LABELS.len());
            if (100..600).contains(&status) {
                let classes = membership(status);
                prop_assert_eq!(classes.iter().filter(|&&yes| yes).count(), 1);
                prop_assert!(classes[index]);
            } else {
                prop_assert_eq!(LABELS[index], "server_error");
            }
        }
    }
}
