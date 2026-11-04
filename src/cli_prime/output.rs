//! The stack-prime/1 presentation contract; no store access lives here.

use std::time::Instant;

use serde_json::{Value, json};

use super::{PRIMER, sections, select};

pub(super) fn envelope(
    text: &str,
    brief: bool,
    summary: Option<&Value>,
    started: Instant,
) -> Value {
    let status = summary
        .and_then(|s| s["status"].as_str())
        .unwrap_or("skipped");
    let mut primer = json!({ "sections": sections(PRIMER) });
    if !brief {
        primer["text"] = json!(text);
    }
    json!({
        "schema": "stack-prime/1",
        "tool": "quipu",
        "version": env!("CARGO_PKG_VERSION"),
        "primer": primer,
        "live": {
            "status": status,
            "elapsed_ms": started.elapsed().as_millis(),
            "summary": summary.cloned().unwrap_or_else(|| json!({})),
        },
        "status": if matches!(status, "ok" | "skipped") { "ok" } else { "degraded" },
    })
}

pub(super) fn brief(summary: Option<&Value>) -> String {
    let mut lines = vec![format!(
        "quipu {}: governed, bitemporal RDF facts queried with SPARQL.",
        env!("CARGO_PKG_VERSION")
    )];
    if let Some(s) = summary {
        let path = s["path"].as_str().unwrap_or("unknown");
        let status = s["status"].as_str().unwrap_or("unreadable");
        // Paths may contain newlines; brief is always at most ten physical lines.
        lines.push(format!("Store: {path:?} ({status})."));
        if matches!(status, "ok" | "partial") {
            let tx = s["last_tx"]["id"]
                .as_i64()
                .map_or_else(|| "unknown".into(), |v| v.to_string());
            lines.push(format!("Last transaction: {tx}."));
            for (key, label) in [
                ("graphs", "Named graphs"),
                ("shapes", "Shapes"),
                ("ontologies", "Ontologies"),
            ] {
                let count = s[key]["count"]
                    .as_i64()
                    .map_or_else(|| "unknown".into(), |v| v.to_string());
                lines.push(format!("{label}: {count}."));
            }
            if status == "partial" {
                lines.push("Summary partial: time budget reached; counts may be unknown.".into());
            }
        } else {
            lines.push("Pass --db <readable-store>, or create a store with `quipu knot`.".into());
            lines.push("Static primer is available offline with `quipu prime --static`.".into());
        }
    } else {
        // Derive the static brief from the single-source What section.
        let what = select(PRIMER, Some("What Quipu is")).expect("primer has a What section");
        lines.extend(
            what.lines()
                .filter(|s| !s.is_empty() && !s.starts_with('#'))
                .take(5)
                .map(str::to_owned),
        );
    }
    lines.push("Run `quipu prime` for the full primer.".into());
    format!("{}\n", lines.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brief_is_bounded_in_every_state_even_with_newlines_in_paths() {
        let summaries = ["ok", "partial", "absent", "unreadable"]
            .map(|status| json!({"status": status, "path": "a\nb\rc"}));
        for summary in std::iter::once(None).chain(summaries.iter().map(Some)) {
            let text = brief(summary);
            assert!((5..=10).contains(&text.lines().count()), "{text}");
            assert!(text.starts_with(concat!("quipu ", env!("CARGO_PKG_VERSION"), ":")));
            assert!(text.ends_with("Run `quipu prime` for the full primer.\n"));
        }
    }

    #[test]
    fn envelope_maps_status_and_omits_brief_text() {
        for status in ["ok", "partial", "absent", "unreadable"] {
            let summary = json!({"status": status, "last_tx": {"id": 42}});
            let v = envelope(PRIMER, true, Some(&summary), Instant::now());
            assert_eq!(v["live"]["summary"], summary);
            assert_eq!(v["live"]["status"], status);
            assert_eq!(v["status"], if status == "ok" { "ok" } else { "degraded" });
            assert!(v["primer"].get("text").is_none());
        }
        let v = envelope(PRIMER, false, None, Instant::now());
        assert_eq!(v["live"]["status"], "skipped");
        assert_eq!(v["status"], "ok");
        assert_eq!(v["primer"]["text"], PRIMER);
    }
}
