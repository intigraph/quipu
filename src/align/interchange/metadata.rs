//! Strict bounded YAML metadata and identifier expansion.
use super::*;
use yaml_rust2::parser::{Event, MarkedEventReceiver, Parser};
use yaml_rust2::scanner::Marker;

#[derive(Default)]
struct Check {
    invalid: bool,
    depth: usize,
}
impl MarkedEventReceiver for Check {
    fn on_event(&mut self, event: Event, _: Marker) {
        match event {
            Event::Alias(_) => self.invalid = true,
            Event::MappingStart(_, _) | Event::SequenceStart(_, _) => {
                self.depth += 1;
                self.invalid |= self.depth > 16;
            }
            Event::MappingEnd | Event::SequenceEnd => self.depth = self.depth.saturating_sub(1),
            _ => {}
        }
    }
}

fn json(yaml: &Yaml) -> Result<Value> {
    Ok(match yaml {
        Yaml::String(s) | Yaml::Real(s) => Value::String(s.clone()),
        Yaml::Integer(n) => Value::from(*n),
        Yaml::Boolean(b) => Value::Bool(*b),
        Yaml::Null => Value::Null,
        Yaml::Array(values) => Value::Array(values.iter().map(json).collect::<Result<_>>()?),
        Yaml::Hash(values) => {
            let mut out = serde_json::Map::new();
            for (key, value) in values {
                let key = key
                    .as_str()
                    .ok_or_else(|| invalid("metadata keys must be strings"))?;
                out.insert(key.into(), json(value)?);
            }
            Value::Object(out)
        }
        _ => return Err(invalid("unsupported YAML value")),
    })
}

pub(super) fn read_metadata(
    text: &str,
) -> Result<(BTreeMap<String, Value>, BTreeMap<String, String>)> {
    // Inspect before YamlLoader clones any aliased collection.
    let mut check = Check::default();
    Parser::new_from_str(text)
        .load(&mut check, true)
        .map_err(|e| invalid(format!("metadata YAML: {e}")))?;
    if check.invalid {
        return Err(invalid("YAML aliases or depth over 16 are not supported"));
    }
    let docs =
        YamlLoader::load_from_str(text).map_err(|e| invalid(format!("metadata YAML: {e}")))?;
    if docs.len() != 1 {
        return Err(invalid("metadata must be exactly one YAML mapping"));
    }
    let Value::Object(object) = json(&docs[0])? else {
        return Err(invalid("metadata must be a mapping"));
    };
    let metadata = object.into_iter().collect();
    let mut prefixes: BTreeMap<String, String> = super::super::export::BUILT_IN_PREFIXES
        .into_iter()
        .map(|(p, iri)| (p.into(), iri.into()))
        .collect();
    if let Some(curie_map) = docs[0]["curie_map"].as_hash() {
        for (key, value) in curie_map {
            let prefix = key.as_str().ok_or_else(|| invalid("non-string prefix"))?;
            let iri = value
                .as_str()
                .ok_or_else(|| invalid("non-string prefix expansion"))?;
            if prefix.is_empty()
                || !prefix
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
                || !prefix.as_bytes()[0].is_ascii_alphabetic()
                || ["http", "https", "urn", "did", "tag", "file", "mailto"].contains(&prefix)
                || iri
                    .split_once(':')
                    .is_some_and(|(scheme, _)| scheme.eq_ignore_ascii_case(prefix))
                || NamedNode::new(iri).is_err()
            {
                return Err(invalid("invalid or IRI-scheme prefix declaration"));
            }
            if prefixes.get(prefix).is_some_and(|old| old != iri) {
                return Err(invalid("built-in prefix redefinition"));
            }
            prefixes.insert(prefix.into(), iri.into());
        }
    } else if !matches!(docs[0]["curie_map"], Yaml::BadValue) {
        return Err(invalid("curie_map must be a mapping"));
    }
    Ok((metadata, prefixes))
}

pub(super) fn expand(value: &str, prefixes: &BTreeMap<String, String>) -> Result<String> {
    if value.trim() != value || value.is_empty() {
        return Err(invalid("empty or padded identifier"));
    }
    let Some((prefix, local)) = value.split_once(':') else {
        return Err(invalid("identifier is not an IRI or declared CURIE"));
    };
    let iri = if let Some(namespace) = prefixes.get(prefix) {
        if local.is_empty() || local.contains('|') {
            return Err(invalid("empty or multi-valued operational identifier"));
        }
        format!("{namespace}{local}")
    } else if ["http", "https", "urn", "did", "tag", "file", "mailto"].contains(&prefix) {
        value.into()
    } else {
        return Err(invalid(format!("undeclared CURIE prefix {prefix:?}")));
    };
    NamedNode::new(&iri).map_err(|_| invalid("invalid expanded IRI"))?;
    Ok(iri)
}
