//! Explicit typed literals for episode properties; legacy scalars stay byte-identical.
use super::{Node, address, escape_turtle, sanitize_iri_local};
use crate::{Error, Result};
use serde_json::Value;

fn scalar(value: &Value) -> Result<Option<String>> {
    Ok(match value {
        Value::String(s) => Some(format!("\"{}\"", escape_turtle(s))),
        Value::Number(n) => n
            .as_i64()
            .map(|i| format!("\"{i}\"^^xsd:integer"))
            .or_else(|| n.as_f64().map(|f| format!("\"{f}\"^^xsd:double"))),
        Value::Bool(b) => Some(format!("\"{b}\"^^xsd:boolean")),
        Value::Object(object) => {
            let lexical = object.get("value").and_then(Value::as_str);
            let datatype = object.get("datatype").and_then(Value::as_str);
            let (Some(lexical), Some(datatype)) = (lexical, datatype) else {
                return Err(Error::InvalidValue(
                    "typed property needs string value and datatype".into(),
                ));
            };
            if object.len() != 2 {
                return Err(Error::InvalidValue(
                    "typed property accepts only value and datatype".into(),
                ));
            }
            address::validate_absolute(datatype)?;
            Some(format!("\"{}\"^^<{datatype}>", escape_turtle(lexical)))
        }
        // Preserve existing null / nested-array behavior.
        _ => None,
    })
}

pub(super) fn render(key: &str, value: &Value) -> Result<Vec<String>> {
    let predicate = if address::absolute_syntax(key) {
        address::validate_absolute(key)?;
        format!("<{key}>")
    } else {
        format!("aegis:{}", sanitize_iri_local(key))
    };
    let values = match value {
        Value::Array(values) => values.as_slice(),
        value => std::slice::from_ref(value),
    };
    values
        .iter()
        .filter_map(|value| match scalar(value) {
            Ok(Some(term)) => Some(Ok(format!(" ;\n    {predicate} {term}"))),
            Ok(None) => None,
            Err(error) => Some(Err(error)),
        })
        .collect()
}

pub(super) fn validate_nodes(nodes: &[Node]) -> Result<()> {
    for node in nodes {
        if let Some(properties) = &node.properties {
            for (key, value) in properties {
                render(key, value)?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn explicit_typed_literal_preserves_predicate_datatype_and_escaping() {
        let value =
            json!({"value": "quote\"\n", "datatype": "http://www.w3.org/2001/XMLSchema#dateTime"});
        let terms = render(
            "https://w3id.org/intigraph/quechua/ns#traceabilityDueAt",
            &value,
        )
        .unwrap();
        assert_eq!(
            terms,
            vec![
                " ;\n    <https://w3id.org/intigraph/quechua/ns#traceabilityDueAt> \"quote\\\"\\n\"^^<http://www.w3.org/2001/XMLSchema#dateTime>"
            ]
        );
        assert_eq!(
            render("count", &json!([1, true, "plain"])).unwrap(),
            vec![
                " ;\n    aegis:count \"1\"^^xsd:integer",
                " ;\n    aegis:count \"true\"^^xsd:boolean",
                " ;\n    aegis:count \"plain\"",
            ]
        );
    }

    #[test]
    fn malformed_typed_objects_and_iri_injection_refuse() {
        for value in [
            json!({"value": "x"}),
            json!({"value": 2, "datatype": "urn:type"}),
            json!({"value": "x", "datatype": "urn:type", "extra": true}),
            json!({"value": "x", "datatype": "urn:x> . <urn:evil>"}),
        ] {
            assert!(render("field", &value).is_err(), "{value}");
        }
        assert!(render("https://example.org/x> . <urn:evil>", &json!("x")).is_err());
    }
}
