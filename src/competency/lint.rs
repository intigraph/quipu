//! Offline syntax and contract consistency checks; no mutation or evaluation.
use super::*;

pub(super) fn scalar(kind: &str, optional: bool) -> Result<&str> {
    let base = if optional {
        kind.strip_suffix('?').unwrap_or(kind)
    } else {
        kind
    };
    if !matches!(
        base,
        "iri" | "string" | "int" | "integer" | "decimal" | "boolean" | "date" | "dateTime"
    ) {
        return Err(invalid(format!("unsupported scalar type {kind:?}")));
    }
    Ok(base)
}

fn names(inputs: &BTreeMap<String, String>) -> Result<()> {
    for (name, kind) in inputs {
        if name.is_empty()
            || !name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')
            || name.as_bytes()[0].is_ascii_digit()
        {
            return Err(invalid(format!("invalid argument/column name {name:?}")));
        }
        scalar(kind, true)?;
    }
    Ok(())
}

fn links(values: &[String]) -> Result<()> {
    if values.is_empty() {
        return Err(invalid("at least one shape/class/policy IRI is required"));
    }
    let mut seen = BTreeSet::new();
    for value in values {
        iri(value)?;
        if !seen.insert(value) {
            return Err(invalid(format!("duplicate target {value}")));
        }
    }
    Ok(())
}

/// Substitute only exact identifier placeholders; SPARQL's group braces remain.
/// Probe values are syntax witnesses, never execution defaults.
fn probe(template: &str, inputs: &BTreeMap<String, String>) -> Result<String> {
    names(inputs)?;
    let mut out = String::new();
    let mut used = BTreeSet::new();
    let mut start = 0;
    while start < template.len() {
        let tail = &template[start..];
        if let Some(rest) = tail.strip_prefix('{')
            && let Some(end) = rest.find('}')
        {
            let name = &rest[..end];
            if !name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
                let kind = inputs
                    .get(name)
                    .ok_or_else(|| invalid(format!("undeclared placeholder {{{name}}}")))?;
                used.insert(name.to_owned());
                out.push_str(match scalar(kind, true)? {
                    "iri" => "urn:quipu:competency:probe",
                    "int" | "integer" | "decimal" => "1",
                    "boolean" => "true",
                    "date" => "2000-01-01",
                    "dateTime" => "2000-01-01T00:00:00Z",
                    _ => "probe",
                });
                start += end + 2;
                continue;
            }
        }
        let ch = tail.chars().next().expect("nonempty tail");
        out.push(ch);
        start += ch.len_utf8();
    }
    for name in inputs.keys() {
        if !used.contains(name) {
            return Err(invalid(format!("input {name} has no placeholder")));
        }
    }
    Ok(out)
}

fn example(inputs: &BTreeMap<String, String>, values: &BTreeMap<String, String>) -> Result<()> {
    for name in values.keys() {
        if !inputs.contains_key(name) {
            return Err(invalid(format!("unknown example input {name}")));
        }
    }
    for (name, kind) in inputs {
        if !kind.ends_with('?') && !values.contains_key(name) {
            return Err(invalid(format!("example missing required input {name}")));
        }
    }
    Ok(())
}

pub(super) fn query_text(
    q: &Question,
    registry: &[crate::share_queries::SharedQuery],
) -> Result<String> {
    match (&q.query, &q.query_ref) {
        (Some(text), None) => Ok(text.clone()),
        (None, Some(name)) => {
            let matches: Vec<_> = registry.iter().filter(|r| r.query.name == *name).collect();
            match matches.as_slice() {
                [query] => Ok(query.query.template.clone()),
                _ => Err(invalid(format!(
                    "query_ref {name:?} must resolve exactly once"
                ))),
            }
        }
        _ => Err(invalid("exactly one of query and query_ref is required")),
    }
}

pub(super) fn question(q: &Question, registry: &[crate::share_queries::SharedQuery]) -> Result<()> {
    if q.ask.trim().is_empty() {
        return Err(invalid("ask must not be empty"));
    }
    links(&q.justifies)?;
    names(&q.returns)?;
    for kind in q.returns.values() {
        scalar(kind, false)?;
    }
    example(&q.inputs, &q.example)?;
    if let Some(shape) = &q.returns_shape {
        iri(shape)?;
    }
    let text = probe(&query_text(q, registry)?, &q.inputs)?;
    let parsed = crate::sparql_structure::parse_query(crate::sparql::sparql_parser(), &text)?
        .map_err(|e| invalid(format!("query does not parse: {e}")))?;
    let ask = matches!(parsed, spargebra::Query::Ask { .. });
    match &parsed {
        spargebra::Query::Select { pattern, .. } => {
            let mut columns = BTreeSet::new();
            pattern.on_in_scope_variable(|v| {
                columns.insert(v.as_str());
            });
            let declared: BTreeSet<_> = q.returns.keys().map(String::as_str).collect();
            if declared.is_empty() || columns != declared {
                return Err(invalid(
                    "SELECT returns must declare exactly the projected columns",
                ));
            }
        }
        spargebra::Query::Construct { .. } if q.returns_shape.is_some() && q.returns.is_empty() => {
        }
        spargebra::Query::Ask { .. } if q.returns.is_empty() && q.returns_shape.is_none() => {}
        _ => {
            return Err(invalid(
                "expected SELECT with returns, CONSTRUCT with returns_shape, or ASK",
            ));
        }
    }
    if let Some(expect) = &q.expect {
        let valid = if ask {
            matches!(expect.as_str(), "ask=true" | "ask=false")
        } else {
            matches!(expect.as_str(), "empty" | "nonempty")
                || expect
                    .strip_prefix("rows=")
                    .is_some_and(|n| n.parse::<u64>().is_ok())
        };
        if !valid {
            return Err(invalid(format!(
                "invalid expectation {expect:?} for query form"
            )));
        }
    }
    Ok(())
}

pub(super) fn command(c: &Command) -> Result<()> {
    if c.does.trim().is_empty() || c.write_class.trim().is_empty() {
        return Err(invalid("does and write_class must not be empty"));
    }
    links(&c.satisfies)?;
    example(&c.inputs, &c.example)?;
    let mut keys = BTreeSet::new();
    if c.key.is_empty() {
        return Err(invalid("key must contain required input names"));
    }
    for key in &c.key {
        if !keys.insert(key) || c.inputs.get(key).is_none_or(|k| k.ends_with('?')) {
            return Err(invalid(format!(
                "key {key:?} must be a unique required input"
            )));
        }
    }
    let text = probe(&c.template, &c.inputs)?;
    let turtle = oxrdfio::RdfParser::from_format(oxrdfio::RdfFormat::Turtle)
        .for_reader(text.as_bytes())
        .collect::<std::result::Result<Vec<_>, _>>();
    if let Ok(quads) = turtle
        && !quads.is_empty()
    {
        return Ok(());
    }
    let update = crate::sparql_structure::parse_update(crate::sparql::sparql_parser(), &text)?
        .map_err(|e| invalid(format!("template is neither Turtle nor INSERT DATA: {e}")))?;
    if update.operations.is_empty() || !update.operations.iter().all(|op| matches!(op, spargebra::GraphUpdateOperation::InsertData { data } if !data.is_empty())) {
        return Err(invalid("command update must contain only nonempty INSERT DATA operations"));
    }
    Ok(())
}
