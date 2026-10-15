//! A deliberately small scheduling profile, never an evaluator or permission.
//! The unchanged query handler still applies graph/label policy and evaluates
//! the original request. Anything outside this grammar uses ordinary admission.

use serde_json::Value;

pub(crate) fn eligible(input: &Value) -> bool {
    let Some(fields) = input.as_object() else {
        return false;
    };
    if fields
        .keys()
        .any(|key| !matches!(key.as_str(), "query" | "verbose" | "_sparql_protocol"))
    {
        return false;
    }
    if ["verbose", "_sparql_protocol"]
        .iter()
        .any(|key| fields.get(*key).is_some_and(|v| !v.is_boolean()))
    {
        return false;
    }
    let Some(text) = input.get("query").and_then(Value::as_str) else {
        return false;
    };
    if text.len() > 1024 || !text.is_ascii() {
        return false;
    }
    let Some(mut rest) = keyword(text.trim(), "ASK") else {
        return false;
    };
    if let Some(tail) = keyword(rest, "WHERE") {
        rest = tail;
    }
    let Some(tail) = rest.strip_prefix('{') else {
        return false;
    };
    rest = tail.trim_start();
    for position in 0..3 {
        let Some((iri, tail)) = iri(rest) else {
            return false;
        };
        // These predicates may have entailment rather than point-lookup
        // semantics. False negatives merely take the ordinary path.
        if position == 1
            && [
                "http://www.w3.org/1999/02/22-rdf-syntax-ns#",
                "http://www.w3.org/2000/01/rdf-schema#",
                "http://www.w3.org/2002/07/owl#",
            ]
            .iter()
            .any(|prefix| iri.starts_with(prefix))
        {
            return false;
        }
        rest = tail.trim_start();
    }
    if let Some(tail) = rest.strip_prefix('.') {
        rest = tail.trim_start();
    }
    rest.strip_prefix('}')
        .is_some_and(|tail| tail.trim().is_empty())
}

fn keyword<'a>(input: &'a str, word: &str) -> Option<&'a str> {
    let prefix = input.get(..word.len())?;
    if !prefix.eq_ignore_ascii_case(word) {
        return None;
    }
    let tail = &input[word.len()..];
    if !tail.is_empty() && !tail.starts_with('{') && !tail.as_bytes()[0].is_ascii_whitespace() {
        return None;
    }
    Some(tail.trim_start())
}

fn iri(input: &str) -> Option<(&str, &str)> {
    let input = input.strip_prefix('<')?;
    let end = input.find('>')?;
    let value = &input[..end];
    if value.bytes().any(|b| {
        b.is_ascii_whitespace()
            || b.is_ascii_control()
            || matches!(b, b'<' | b'"' | b'{' | b'}' | b'|' | b'^' | b'`' | b'\\')
    }) {
        return None;
    }
    oxrdf::NamedNode::new(value).ok()?;
    Some((value, &input[end + 1..]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const POINT: &str =
        "ASK { <https://example.test/s> <https://example.test/p> <https://example.test/o> }";

    #[test]
    fn only_bounded_ground_point_asks_use_the_reserve() {
        assert!(eligible(&json!({"query":POINT})));
        assert!(eligible(
            &json!({"query":POINT.replace("ASK {", "aSk WHERE {").replace(" }", " . }"), "verbose":true, "_sparql_protocol":true})
        ));
        for query in [
            "ASK { ?s ?p ?o }",
            "ASK { <https://example.test/s> <https://example.test/p> ?o }",
            "ASK { <https://example.test/s> <https://example.test/p>* <https://example.test/o> }",
            "ASK { <https://example.test/s> <https://example.test/p> <https://example.test/o> . ?s ?p ?o }",
            "ASK { SERVICE <https://example.test/remote> { ?s ?p ?o } }",
            "ASK { <https://example.test/s> a <https://example.test/o> }",
            "ASKWHERE { <https://example.test/s> <https://example.test/p> <https://example.test/o> }",
            "ASK { <relative> <https://example.test/p> <https://example.test/o> }",
            "ASK { <https://example.test/s> <https://example.test/p> <https://example.test/o> } LIMIT 1",
            "ASK { <https://example.test/s> <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <https://example.test/o> }",
        ] {
            assert!(!eligible(&json!({"query":query})), "{query}");
        }
        for field in [
            "tx",
            "valid_at",
            "graph",
            "row_labels",
            "federated",
            "priority",
        ] {
            let mut input = json!({"query":POINT});
            input[field] = json!(true);
            assert!(!eligible(&input), "{field}");
        }
        assert!(!eligible(
            &json!({"query":format!("{}{}", " ".repeat(1024), POINT)})
        ));
        assert!(!eligible(&json!({"query":POINT.replace("/s>", "/é>" )})));
    }
}
