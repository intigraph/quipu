//! Read-only parser guidance. Never supplies a namespace or rewrites a query.
use crate::error::Error;
use std::collections::BTreeSet;

/// Preserve the parser verdict while identifying undeclared prefix names.
pub fn query_error(text: &str, error: &spargebra::SparqlSyntaxError) -> Error {
    let original = format!("SPARQL parse error: {error}");
    if !original.contains("Prefix not found") {
        return Error::InvalidValue(original);
    }
    let names = missing_prefixes(text);
    let named = if names.is_empty() {
        "Declare each prefixed name".to_string()
    } else {
        format!(
            "Undeclared prefix(es): {}",
            names
                .iter()
                .map(|p| format!("{p}: (PREFIX {p}: <intended-namespace-IRI>)"))
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    Error::InvalidValue(format!(
        "SPARQL parse error: {named}; include explicit PREFIX declarations. No namespace is assumed or inserted. Parser verdict: {error}"
    ))
}

fn missing_prefixes(text: &str) -> Vec<String> {
    let bytes = text.as_bytes();
    let (mut used, mut declared) = (BTreeSet::new(), BTreeSet::new());
    let (mut i, mut declaration) = (0, false);
    while i < bytes.len() {
        match bytes[i] {
            b'#' => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            q @ (b'\'' | b'"') => {
                i = super::skip_string(bytes, i, q);
            }
            b'<' if super::iri_end(bytes, i).is_some() => {
                i = super::iri_end(bytes, i).unwrap() + 1;
            }
            b'?' | b'$' => {
                i += 1;
                while i < bytes.len() && name(bytes[i]) {
                    i += 1;
                }
            }
            c if name(c) || c == b':' => {
                let start = i;
                while i < bytes.len() && name(bytes[i]) {
                    i += 1;
                }
                if bytes.get(i) == Some(&b':') {
                    let prefix = &text[start..i];
                    if prefix.len() <= 64 {
                        if declaration {
                            declared.insert(prefix.to_owned());
                        } else {
                            used.insert(prefix.to_owned());
                        }
                    }
                    declaration = false;
                    i += 1;
                    while i < bytes.len() {
                        if bytes[i] == b'\\' && i + 1 < bytes.len() {
                            i += 2;
                        } else if super::is_local(bytes[i]) {
                            i += 1;
                        } else {
                            break;
                        }
                    }
                } else {
                    declaration = text[start..i].eq_ignore_ascii_case("PREFIX");
                }
            }
            _ => {
                i += 1;
            }
        }
    }
    used.difference(&declared).take(8).cloned().collect()
}
fn name(c: u8) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-' | b'.') || c >= 0x80
}

#[cfg(test)]
mod tests {
    use super::*;
    fn diagnostic(q: &str) -> String {
        let error = crate::sparql::sparql_parser().parse_query(q).unwrap_err();
        query_error(q, &error).to_string()
    }
    #[test]
    fn missing_prefix_is_named_without_changing_the_query() {
        let q = "SELECT ?s WHERE { ?s rdfs:label ?l }";
        let out = diagnostic(q);
        assert!(out.starts_with("SPARQL parse error: Undeclared prefix(es): rdfs:"));
        assert!(out.contains("No namespace is assumed"));
        assert!(crate::sparql::sparql_parser().parse_query(q).is_err());
    }
    #[test]
    fn comments_strings_iris_and_declared_custom_names_are_not_missing() {
        let q = "PREFIX own: <https://example.test/> SELECT ?s WHERE { ?s own:p 'literal:thing' ; missing:p <https://example.test/a:b> } # ignored:thing\n";
        assert_eq!(missing_prefixes(q), vec!["missing"]);
        assert_eq!(
            missing_prefixes("PREFIX : <https://example.test/> ASK { :s :p :o }"),
            Vec::<String>::new()
        );
        assert_eq!(missing_prefixes("ASK { :s :p :o }"), vec![""]);
        assert_eq!(
            missing_prefixes("SELECT ?x WHERE { ?x p:a\\:b ?v }"),
            vec!["p"]
        );
    }
    #[test]
    fn unrelated_syntax_errors_keep_the_original_message() {
        let q = "SELECT WHERE {";
        let e = crate::sparql::sparql_parser().parse_query(q).unwrap_err();
        assert_eq!(
            query_error(q, &e).to_string(),
            format!("SPARQL parse error: {e}")
        );
    }
}

#[cfg(test)]
mod contract_tests {
    use super::*;
    #[test]
    fn all_common_prefixes_need_explicit_declarations_and_valid_queries_stay_valid() {
        for prefix in ["rdf", "rdfs", "xsd", "schema", "prov", "aegis", "quechua"] {
            let q = format!("ASK {{ ?s {prefix}:term ?o }}");
            let error = crate::sparql::sparql_parser().parse_query(&q).unwrap_err();
            assert!(
                query_error(&q, &error)
                    .to_string()
                    .contains(&format!("Undeclared prefix(es): {prefix}:"))
            );
            let explicit = format!("PREFIX {prefix}: <https://example.test/ns#> {q}");
            assert!(
                crate::sparql::sparql_parser()
                    .parse_query(&explicit)
                    .is_ok()
            );
        }
    }
    #[test]
    fn timeout_and_native_manifest_steer_discovery_without_weakening_authority() {
        let text = Error::QueryTimeout {
            elapsed_ms: 11,
            limit_ms: 10,
        }
        .to_string();
        assert!(text.contains("quipu_search"));
        assert!(text.contains("not an authoritative"));
        for scope in ["exact-ID", "owner-plate", "named-graph", "absence"] {
            assert!(text.contains(scope));
        }
        let query = crate::tool_definitions()
            .into_iter()
            .find(|v| v["name"] == "quipu_query")
            .unwrap();
        assert!(
            query["description"]
                .as_str()
                .unwrap()
                .contains("explicit PREFIX")
        );
    }
}
