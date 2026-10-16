//! Suggestions remain advisory: only the loaded vocabulary can sanction types.

use std::collections::BTreeSet;

pub(super) fn hint(unknown: &[String], vocabulary: &BTreeSet<String>) -> String {
    // Bounded local names only: namespace similarity is not class similarity.
    let locals: Vec<_> = unknown.iter().take(8).map(|iri| local(iri)).collect();
    let mut ranked: Vec<_> = vocabulary
        .iter()
        .filter_map(|iri| {
            let candidate = local(iri);
            if candidate.is_empty() {
                return None;
            }
            let score = locals
                .iter()
                .filter(|name| !name.is_empty())
                .map(|name| strsim::jaro_winkler(name, &candidate))
                .fold(0.0_f64, f64::max);
            (score >= 0.85).then_some((score, iri))
        })
        .collect();
    ranked.sort_by(|a, b| b.0.total_cmp(&a.0).then_with(|| a.1.cmp(b.1)));
    let candidates: Vec<_> = ranked
        .into_iter()
        .take(3)
        .map(|(_, iri)| iri.as_str())
        .collect();
    if candidates.is_empty() {
        return String::new();
    }
    format!(
        " Similar loaded class IRIs: {} (lexical similarity only, not semantic equivalence; inspect the shapes before choosing, or propose a new shape if none fits).",
        candidates.join(", ")
    )
}

fn local(iri: &str) -> String {
    let name = iri.rsplit(['#', '/', ':']).next().unwrap_or("");
    if name.len() > 128 {
        return String::new();
    }
    name.to_lowercase()
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    #[test]
    fn native_refusal_names_loaded_candidates_without_writing_or_retyping() {
        let mut store = crate::Store::open_in_memory().unwrap();
        crate::tool_shapes(&store, &json!({"action":"load","name":"fixture","turtle":"@prefix sh: <http://www.w3.org/ns/shacl#> . <https://example.test/Shape> a sh:NodeShape ; sh:targetClass <https://example.test/Role> ."})).unwrap();
        let error = crate::tool_knot(
            &mut store,
            &json!({"turtle":"<https://example.test/bad> a <urn:fixture:job#Role> ."}),
        )
        .unwrap_err();
        let absent = crate::tool_query(
            &store,
            &json!({"query":"ASK { <https://example.test/bad> ?p ?o }"}),
        )
        .unwrap();
        assert_eq!(absent["result"], false);
        crate::tool_knot(
            &mut store,
            &json!({"turtle":"<https://example.test/good> a <https://example.test/Role> ."}),
        )
        .unwrap();
        let positive = crate::tool_query(
            &store,
            &json!({"query":"ASK { <https://example.test/good> a <https://example.test/Role> }"}),
        )
        .unwrap();
        assert_eq!(positive["result"], true);
        eprintln!("refused/no-write with legal-type positive: {error}");
        let text = error.to_string();
        assert!(text.contains("Similar loaded class IRIs: https://example.test/Role"));
        assert!(text.contains("not semantic equivalence"));
    }
}

#[cfg(test)]
mod bounds_tests {
    use super::*;
    #[test]
    fn suggestions_are_loaded_only_capped_and_deterministic() {
        let loaded: BTreeSet<_> = [
            "https://example.test/a/Role",
            "https://example.test/b/Role",
            "https://example.test/c/Role",
            "https://example.test/d/Role",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();
        let out = hint(&["urn:fixture:Role".into()], &loaded);
        assert!(out.contains("https://example.test/a/Role"));
        assert!(!out.contains("https://example.test/d/Role"));
        assert!(!out.contains("Person"));
        assert_eq!(out, hint(&["urn:fixture:Role".into()], &loaded));
        assert_eq!(hint(&["urn:fixture:Role".into()], &BTreeSet::new()), "");
        assert_eq!(
            hint(&[format!("urn:fixture:{}", "r".repeat(1000))], &loaded),
            ""
        );
    }
}
