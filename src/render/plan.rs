use super::{MAX_BYTES, MAX_FILES, RenderRequest, check_path, invalid, spec};
use crate::{Result, Store};
use base64::{Engine, engine::general_purpose::STANDARD};
use minijinja::{Environment, UndefinedBehavior};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::io::Write;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RenderFile {
    pub sha256: String,
    pub base64: String,
    pub mode: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RenderPlan {
    pub schema: u32,
    pub spec: String,
    pub source_tx: i64,
    pub valid_at: Option<String>,
    pub files: BTreeMap<String, RenderFile>,
}

/// Format seam. Future formats consume the same bounded JSON context and
/// publication contract; binary postprocessing is outside this trait.
pub trait Renderer {
    fn render(&self, template: Option<&str>, context: &Value) -> Result<Vec<u8>>;
}
pub struct Markdown;

struct Bounded(Vec<u8>);
impl Write for Bounded {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if self.0.len().saturating_add(bytes.len()) > MAX_BYTES {
            return Err(std::io::Error::other("render output cap exceeded"));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn template_bytes(template: &str, context: &Value) -> Result<Vec<u8>> {
    if template.len() > 256 * 1024 {
        return Err(invalid("template too large"));
    }
    let tags = regex::Regex::new(r"(?s)\{[\{%](.*?)[%\}]\}").expect("constant regex");
    if tags.captures_iter(template).any(|tag| tag[1].contains('*')) {
        return Err(invalid(
            "multiplication/repetition is unsupported in template expressions",
        ));
    }
    let mut env = Environment::empty();
    env.add_filter("upper", |value: String| value.to_uppercase());
    env.add_filter("lower", |value: String| value.to_lowercase());
    // No loader, custom object, callback, filesystem or network function is
    // installed. Remove allocation-prone range; loops only see bounded rows.
    env.set_fuel(Some(100_000));
    env.set_undefined_behavior(UndefinedBehavior::Strict);
    env.add_template("document", template)
        .map_err(|e| invalid(e.to_string()))?;
    let mut output = Bounded(Vec::new());
    env.get_template("document")
        .map_err(|e| invalid(e.to_string()))?
        .render_captured_to(context, &mut output)
        .map_err(|e| invalid(e.to_string()))?;
    Ok(output.0)
}

impl Renderer for Markdown {
    fn render(&self, template: Option<&str>, context: &Value) -> Result<Vec<u8>> {
        if let Some(template) = template {
            return template_bytes(template, context);
        }
        let rows = context["rows"]
            .as_array()
            .ok_or_else(|| invalid("missing rows"))?;
        let mut text = String::new();
        for row in rows {
            text.push_str("## Result\n\n");
            let object = row
                .as_object()
                .ok_or_else(|| invalid("row must be an object"))?;
            for (key, value) in object {
                text.push_str(&format!(
                    "- **{key}**: {}\n",
                    value
                        .as_str()
                        .map_or_else(|| value.to_string(), String::from)
                ));
            }
            text.push('\n');
        }
        Ok(text.into_bytes())
    }
}

pub(crate) fn add_file(
    files: &mut BTreeMap<String, RenderFile>,
    path: String,
    bytes: &[u8],
    mode: u32,
) -> Result<()> {
    check_path(&path)?;
    if !matches!(mode, 0o644 | 0o755) {
        return Err(invalid("only modes 0644 and 0755 supported"));
    }
    if files.len() >= MAX_FILES || bytes.len() > MAX_BYTES {
        return Err(invalid("file/output cap exceeded"));
    }
    for existing in files.keys() {
        let old = existing.to_ascii_lowercase();
        let new = path.to_ascii_lowercase();
        let directory_alias = existing
            .split('/')
            .zip(path.split('/'))
            .take_while(|(a, b)| a.eq_ignore_ascii_case(b))
            .any(|(a, b)| a != b);
        if old == new
            || old.starts_with(&format!("{new}/"))
            || new.starts_with(&format!("{old}/"))
            || directory_alias
        {
            return Err(invalid(format!("colliding output path: {path}")));
        }
    }
    files.insert(
        path,
        RenderFile {
            sha256: crate::share::sha256(bytes),
            base64: STANDARD.encode(bytes),
            mode,
        },
    );
    Ok(())
}

fn path(template: &str, context: &Value) -> Result<String> {
    // Path templates use the same strict deterministic language as documents;
    // braces {slug} are accepted as the design's convenient shorthand.
    let expr = regex::Regex::new(r"\{([A-Za-z_][A-Za-z0-9_]*)\}").expect("constant regex");
    let template = expr.replace_all(template, "{{ row.$1 }}");
    let bytes = template_bytes(&template, context)?;
    let path = String::from_utf8(bytes).map_err(|_| invalid("path must be UTF-8"))?;
    check_path(&path)?;
    Ok(path)
}

/// Produce a complete plan from ONE SQLite read snapshot. No graph write,
/// network federation or disk destination is involved.
pub fn prepare(store: &Store, request: &RenderRequest) -> Result<RenderPlan> {
    prepare_with_interleave(store, request, || Ok(()))
}

// The private seam allows a deterministic WAL writer between the first read
// and document evaluation. Production installs no callback or external I/O.
pub(super) fn prepare_with_interleave(
    store: &Store,
    request: &RenderRequest,
    after_snapshot: impl FnOnce() -> Result<()>,
) -> Result<RenderPlan> {
    if store.has_attachments() {
        return Err(invalid(
            "attached stores have no single source transaction; unsupported in v1",
        ));
    }
    if let Some(at) = &request.valid_at
        && crate::time::epoch_of_rfc3339(at).is_none()
    {
        return Err(invalid("valid-at must be an ISO timestamp"));
    }
    let snapshot = store.conn.unchecked_transaction()?;
    let source_tx = store.latest_tx_id()?;
    after_snapshot()?;
    let (identity, spec) = spec::resolve(store, request, source_tx)?;
    if spec.format != "markdown" || !matches!(spec.layout.as_str(), "single" | "per-entity") {
        return Err(invalid(
            "v1 requires markdown and single or per-entity layout",
        ));
    }
    // Conservative refusal includes occurrences in literals: v1 promises no
    // network and deterministic bytes, rather than guessing about extensions.
    let unsafe_query = regex::Regex::new(
        r"(?i)\bSERVICE\b|\b(NOW|RAND|UUID|STRUUID|BNODE|SAMPLE|GROUP_CONCAT)\s*\(|_:",
    )
    .expect("constant regex");
    if unsafe_query.is_match(&spec.query) {
        return Err(invalid(
            "query contains network, blank-node or nondeterministic constructs",
        ));
    }
    // Native query boundary preserves configured floors, dataset selection and
    // budget. Never turn truncated or UNKNOWN answers into an output deletion.
    let mut input = spec::query_input(&spec.query, request, source_tx);
    if let Some(at) = &request.valid_at {
        input["valid_at"] = json!(at);
    }
    if let Some(graph) = &request.graph {
        input["graph"] = json!(graph);
    }
    let result = crate::tool_query(store, &input)?;
    if result["truncated"].as_bool() != Some(false) {
        return Err(invalid("query incomplete or not SELECT/CONSTRUCT"));
    }
    let mut rows = result
        .get("rows")
        .or_else(|| result.get("triples"))
        .and_then(Value::as_array)
        .ok_or_else(|| invalid("query must SELECT or CONSTRUCT"))?
        .clone();
    if rows.len() > MAX_FILES {
        return Err(invalid("row cap exceeded"));
    }
    rows.sort_by_cached_key(Value::to_string);
    if serde_json::to_vec(&rows)
        .map_err(|e| invalid(e.to_string()))?
        .len()
        > MAX_BYTES
    {
        return Err(invalid("query context cap exceeded"));
    }
    let mut files = BTreeMap::new();
    let groups: Vec<Vec<Value>> = if spec.layout == "single" {
        vec![rows]
    } else {
        rows.into_iter().map(|row| vec![row]).collect()
    };
    for rows in groups {
        let row = rows.first().cloned().unwrap_or_else(|| json!({}));
        let context = json!({"rows":rows,"row":row,"source_spec":identity,"source_tx":source_tx,"valid_at":request.valid_at});
        let output = path(&spec.path, &context)?;
        let mut bytes = Markdown.render(spec.template.as_deref(), &context)?;
        if spec.provenance {
            bytes.extend_from_slice(
                format!(
                    "\n<!-- quipu spec={} tx={} valid-at={} -->\n",
                    identity.replace("--", ""),
                    source_tx,
                    request.valid_at.as_deref().unwrap_or("present")
                )
                .as_bytes(),
            );
        }
        add_file(&mut files, output, &bytes, spec::default_mode())?;
        let mut includes = spec.includes.clone();
        if spec.layout == "per-entity"
            && let Some(data) = context["row"].get("includes")
        {
            let extra: Vec<super::Include> = if let Some(text) = data.as_str() {
                serde_json::from_str(text)
            } else {
                serde_json::from_value(data.clone())
            }
            .map_err(|e| invalid(format!("row includes: {e}")))?;
            includes.extend(extra);
        }
        for include in &includes {
            if include.base64.len() > MAX_BYTES * 2 {
                return Err(invalid("include cap exceeded"));
            }
            let bytes = STANDARD
                .decode(&include.base64)
                .map_err(|_| invalid("invalid include base64"))?;
            if crate::share::sha256(&bytes) != include.sha256 {
                return Err(invalid("include content hash mismatch"));
            }
            add_file(
                &mut files,
                path(&include.path, &context)?,
                &bytes,
                include.mode,
            )?;
        }
    }
    let plan = RenderPlan {
        schema: 1,
        spec: identity,
        source_tx,
        valid_at: request.valid_at.clone(),
        files,
    };
    plan.validate()?;
    snapshot.rollback()?;
    Ok(plan)
}

impl RenderPlan {
    /// Also check untrusted remote plans before touching a destination.
    pub fn validate(&self) -> Result<()> {
        if self.schema != 1 || self.source_tx < 0 || self.files.len() > MAX_FILES {
            return Err(invalid("invalid plan identity/cap"));
        }
        let mut checked = BTreeMap::new();
        let mut total = 0usize;
        for (path, file) in &self.files {
            if file.base64.len() > MAX_BYTES * 2 {
                return Err(invalid("encoded file cap exceeded"));
            }
            let bytes = STANDARD
                .decode(&file.base64)
                .map_err(|_| invalid("invalid plan base64"))?;
            total = total.saturating_add(bytes.len());
            if total > MAX_BYTES || crate::share::sha256(&bytes) != file.sha256 {
                return Err(invalid("plan content hash/cap mismatch"));
            }
            add_file(&mut checked, path.clone(), &bytes, file.mode)?;
        }
        Ok(())
    }
}
