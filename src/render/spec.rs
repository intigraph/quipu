use super::{NS, SPEC_TYPE, invalid};
use crate::{Result, Store};
use serde::{Deserialize, Serialize};

/// Content-addressed bytes travel with the specification as RDF literal data,
/// so a pendant does not depend on the producer's filesystem.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Include {
    pub path: String,
    pub sha256: String,
    pub base64: String,
    #[serde(default = "default_mode")]
    pub mode: u32,
}
pub(crate) fn default_mode() -> u32 {
    0o644
}
fn markdown() -> String {
    "markdown".into()
}
fn single() -> String {
    "single".into()
}
fn output_path() -> String {
    "report.md".into()
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RenderSpec {
    pub query: String,
    #[serde(default = "markdown")]
    pub format: String,
    #[serde(default = "single")]
    pub layout: String,
    #[serde(default = "output_path")]
    pub path: String,
    pub template: Option<String>,
    #[serde(default)]
    pub includes: Vec<Include>,
    #[serde(default)]
    pub provenance: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RenderRequest {
    pub spec: Option<String>,
    pub adhoc: Option<RenderSpec>,
    pub valid_at: Option<String>,
    pub graph: Option<String>,
}

pub(crate) fn resolve(
    store: &Store,
    request: &RenderRequest,
    source_tx: i64,
) -> Result<(String, RenderSpec)> {
    match (&request.spec, &request.adhoc) {
        (None, Some(spec)) => Ok(("adhoc".into(), spec.clone())),
        (Some(iri), None) => {
            oxrdf::NamedNode::new(iri.clone())
                .map_err(|_| invalid("spec must be an absolute IRI"))?;
            let query = format!("SELECT ?p ?v WHERE {{ <{iri}> ?p ?v }}");
            let mut input = query_input(&query, request, source_tx);
            if let Some(at) = &request.valid_at {
                input["valid_at"] = serde_json::json!(at);
            }
            if let Some(graph) = &request.graph {
                input["graph"] = serde_json::json!(graph);
            }
            let result = crate::tool_query(store, &input)?;
            if result["truncated"].as_bool() != Some(false) {
                return Err(invalid("incomplete stored specification"));
            }
            let mut props = serde_json::Map::new();
            let mut governed = false;
            for row in result["rows"]
                .as_array()
                .ok_or_else(|| invalid("invalid spec result"))?
            {
                let predicate = row["p"]
                    .as_str()
                    .ok_or_else(|| invalid("invalid spec predicate"))?;
                let value = row.get("v").ok_or_else(|| invalid("missing spec value"))?;
                if predicate == "http://www.w3.org/1999/02/22-rdf-syntax-ns#type" {
                    governed |= value.as_str() == Some(SPEC_TYPE);
                    continue;
                }
                let Some(key) = predicate.strip_prefix(NS) else {
                    continue;
                };
                if !matches!(
                    key,
                    "query" | "format" | "layout" | "path" | "template" | "includes" | "provenance"
                ) {
                    return Err(invalid(format!("unknown spec field {key}")));
                }
                let json = if key == "includes" {
                    let Some(text) = value.as_str() else {
                        return Err(invalid("includes must be JSON string"));
                    };
                    serde_json::from_str(text).map_err(|e| invalid(format!("includes: {e}")))?
                } else {
                    value.clone()
                };
                if props.insert(key.into(), json).is_some() {
                    return Err(invalid(format!("duplicate spec field {key}")));
                }
            }
            if !governed {
                return Err(invalid("missing RenderSpec type in selected temporal view"));
            }
            let spec = serde_json::from_value(serde_json::Value::Object(props))
                .map_err(|e| invalid(format!("invalid stored spec: {e}")))?;
            Ok((iri.clone(), spec))
        }
        _ => Err(invalid(
            "choose exactly one stored spec or ad-hoc specification",
        )),
    }
}

/// Use the native read pin where admitted. Historical valid time retains the
/// single SQLite read snapshot: the service refuses mixed `tx`/`valid_at` clocks.
pub(crate) fn query_input(
    query: &str,
    request: &RenderRequest,
    source_tx: i64,
) -> serde_json::Value {
    let mut input = serde_json::json!({"query":query,"verbose":true});
    if request.valid_at.is_none() && source_tx > 0 {
        input["tx"] = serde_json::json!(source_tx);
    }
    input
}
