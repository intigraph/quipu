//! DataBook documents: Markdown with YAML frontmatter and typed fenced blocks
//! (`kurtcagle/databook`, release 1.5.0, the W3C Holon CG format).
//!
//! This module is the store-free half: it reads a DataBook into a header and
//! its blocks, and writes the frontmatter and fences an exporter assembles.
//! [`crate::databook_store`] moves graphs between a [`crate::Store`] and a
//! DataBook.
//!
//! Reading accepts both frontmatter forms (bare `---`, canonical since 1.1, and
//! the 1.0 `<script language="application/yaml">` wrapper) and both block
//! annotation placements: before the fence (the 1.5 reference parser's
//! canonical form) and as the first lines inside it (SPEC §6.2). When a key
//! appears in both places the pre-fence value wins, as in the reference
//! parser. Writing always puts annotations before the fence, so each fence
//! holds nothing but its payload.

use std::collections::BTreeMap;

use oxrdfio::RdfFormat;
use yaml_rust2::{Yaml, YamlLoader};

use crate::error::{Error, Result};

/// The document types SPEC §5.1 recognises. Anything else is not a DataBook.
pub const DOCUMENT_TYPES: [&str; 3] = ["databook", "transformer-library", "processor-registry"];

/// Which frontmatter form the document used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrontmatterForm {
    /// Bare `---` frontmatter at the document start.
    Canonical,
    /// The 1.0 `<script language="application/yaml">` wrapper.
    Script,
}

/// The frontmatter fields a reader acts on. Unknown keys are ignored (§15.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    pub id: String,
    pub title: String,
    pub doc_type: String,
    pub version: String,
    pub created: Option<String>,
    /// `graph.named_graph`: where blocks without `databook:graph` load.
    pub named_graph: Option<String>,
    /// `graph.triple_count`, an informational integrity check (§5.3).
    pub triple_count: Option<u64>,
    pub has_process: bool,
}

/// What a fenced block is, for a reader that loads data and never executes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockKind {
    /// Graph data in an RDF syntax Quipu parses.
    Rdf(RdfFormat),
    /// A `shacl` block: shapes in Turtle.
    Shapes,
    /// A `sparql` query.
    Query,
    /// A `sparql-update`, which an importer must never run.
    Update,
    /// An encryption-profile block, opaque without that profile (§6.1).
    Opaque,
    /// A recognised payload this reader does not load (JSON-LD, manifests...).
    Unsupported,
    /// A display block: code, prose fences, unknown labels, `mode=printed`.
    Display,
}

/// One fenced block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    /// The fence label (`turtle`, `sparql`, ...).
    pub label: String,
    /// `databook:` keys stored bare (`id`, `graph`); other prefixes keep theirs.
    pub meta: BTreeMap<String, String>,
    /// Pre-fence `key=value` directives (`mode=printed`).
    pub directives: BTreeMap<String, String>,
    /// The payload, with inline annotation lines removed.
    pub payload: String,
    /// 1-based line of the opening fence, for messages.
    pub line: usize,
}

impl Block {
    /// The `databook:id`, if any.
    pub fn id(&self) -> Option<&str> {
        self.meta.get("id").map(String::as_str)
    }

    /// Marked for reading only: `mode=printed|hidden|reference`, or the older
    /// `databook:display-only: true`. Such a block is still part of the
    /// document's data; a loader just leaves it alone.
    pub fn printed(&self) -> bool {
        self.meta.get("display-only").is_some_and(|v| v == "true")
            || self
                .directives
                .get("mode")
                .is_some_and(|m| matches!(m.as_str(), "printed" | "hidden" | "reference"))
    }

    /// What the fence label says the payload is, ignoring [`Block::printed`].
    pub fn label_kind(&self) -> BlockKind {
        match self.label.as_str() {
            // Manifests, transformer libraries and processor registries are
            // RDF graphs in Turtle (SPEC §6.1, §11, §12).
            "turtle" | "turtle12" | "manifest" | "transformer-library" | "processor-registry" => {
                BlockKind::Rdf(RdfFormat::Turtle)
            }
            "n-triples" => BlockKind::Rdf(RdfFormat::NTriples),
            "trig" => BlockKind::Rdf(RdfFormat::TriG),
            "n-quads" => BlockKind::Rdf(RdfFormat::NQuads),
            "shacl" => BlockKind::Shapes,
            "sparql" => BlockKind::Query,
            "sparql-update" => BlockKind::Update,
            label if label.starts_with("encrypted") => BlockKind::Opaque,
            "json-ld" => BlockKind::Unsupported,
            _ => BlockKind::Display,
        }
    }

    /// How a loading reader should treat this block: a printed block is
    /// [`BlockKind::Display`] whatever its label.
    pub fn kind(&self) -> BlockKind {
        if self.printed() {
            BlockKind::Display
        } else {
            self.label_kind()
        }
    }
}

/// A parsed DataBook.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataBook {
    pub header: Header,
    pub form: FrontmatterForm,
    pub blocks: Vec<Block>,
    /// §15.4 conditions that are warnings, not errors.
    pub warnings: Vec<String>,
}

fn invalid(message: impl Into<String>) -> Error {
    Error::InvalidValue(format!("databook: {}", message.into()))
}

/// Parse a DataBook. Errors follow SPEC §15.4: no or malformed frontmatter,
/// a missing identity field, an unrecognised `type`, or no fenced data block.
pub fn parse(text: &str) -> Result<DataBook> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    let lines: Vec<&str> = text.split('\n').collect();
    let (yaml, body_start, form) = frontmatter(&lines)?;
    let docs = YamlLoader::load_from_str(&yaml)
        .map_err(|e| invalid(format!("malformed YAML frontmatter: {e}")))?;
    let doc = docs.into_iter().next().unwrap_or(Yaml::Null);
    if doc.as_hash().is_none() {
        return Err(invalid("frontmatter is not a YAML mapping"));
    }
    let required = |key: &str| {
        scalar(&doc[key])
            .filter(|v| !v.trim().is_empty())
            .ok_or_else(|| invalid(format!("missing required frontmatter field `{key}`")))
    };
    let header = Header {
        id: required("id")?,
        title: required("title")?,
        doc_type: required("type")?,
        version: required("version")?,
        created: scalar(&doc["created"]),
        named_graph: scalar(&doc["graph"]["named_graph"]),
        triple_count: doc["graph"]["triple_count"]
            .as_i64()
            .and_then(|n| u64::try_from(n).ok()),
        has_process: doc["process"].as_hash().is_some(),
    };
    if !DOCUMENT_TYPES.contains(&header.doc_type.as_str()) {
        return Err(invalid(format!(
            "type `{}` is not a DataBook type ({})",
            header.doc_type,
            DOCUMENT_TYPES.join(", ")
        )));
    }
    let blocks = blocks(&lines, body_start)?;
    if blocks.iter().all(|b| b.label_kind() == BlockKind::Display) {
        return Err(invalid(
            "no fenced data block; a DataBook needs at least one",
        ));
    }
    let mut warnings = Vec::new();
    if header.created.is_none() {
        warnings.push("missing `created` date".to_string());
    }
    if !header.has_process {
        warnings.push("no `process` stamp: provenance is incomplete".to_string());
    }
    Ok(DataBook {
        header,
        form,
        blocks,
        warnings,
    })
}

/// A YAML scalar as text. Numbers keep their written form (`1.0` stays `1.0`).
fn scalar(yaml: &Yaml) -> Option<String> {
    match yaml {
        Yaml::String(s) | Yaml::Real(s) => Some(s.clone()),
        Yaml::Integer(i) => Some(i.to_string()),
        Yaml::Boolean(b) => Some(b.to_string()),
        _ => None,
    }
}

fn is_delimiter(line: &str) -> bool {
    line.trim_end() == "---"
}

/// Find the frontmatter: `(yaml, first body line, form)`.
fn frontmatter(lines: &[&str]) -> Result<(String, usize, FrontmatterForm)> {
    let first = lines.iter().position(|l| !l.trim().is_empty());
    if let Some(start) = first.filter(|&i| is_delimiter(lines[i])) {
        let end = (start + 1..lines.len())
            .find(|&i| is_delimiter(lines[i]))
            .ok_or_else(|| invalid("frontmatter opened with `---` is never closed"))?;
        let yaml = lines[start + 1..end].join("\n");
        return Ok((yaml, end + 1, FrontmatterForm::Canonical));
    }
    let open = lines.iter().position(|l| {
        let l = l.trim();
        l.starts_with("<script")
            && (l.contains("language=\"application/yaml\"")
                || l.contains("language='application/yaml'"))
    });
    if let Some(open) = open {
        let close = (open + 1..lines.len())
            .find(|&i| lines[i].trim() == "</script>")
            .ok_or_else(|| invalid("<script> frontmatter is never closed"))?;
        let inner = &lines[open + 1..close];
        let delims: Vec<usize> = (0..inner.len())
            .filter(|&i| is_delimiter(inner[i]))
            .collect();
        let yaml = match delims.as_slice() {
            [a, b, ..] => inner[a + 1..*b].join("\n"),
            [a] => inner[a + 1..].join("\n"),
            [] => inner.join("\n"),
        };
        return Ok((yaml, close + 1, FrontmatterForm::Script));
    }
    Err(invalid("no YAML frontmatter; this is not a DataBook"))
}

/// `<!-- prefix:key: value -->` as `(stored key, value)`; `databook:` is bare.
fn annotation(line: &str) -> Option<(String, String)> {
    let inner = line
        .trim()
        .strip_prefix("<!--")?
        .strip_suffix("-->")?
        .trim();
    let (prefix, rest) = inner.split_once(':')?;
    let (key, value) = rest.split_once(':')?;
    let word = |s: &str| {
        !s.is_empty()
            && s.chars()
                .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
    };
    if !word(prefix) || !word(key) || value.chars().next().is_some_and(|c| !c.is_whitespace()) {
        return None;
    }
    let stored = if prefix == "databook" {
        key.to_string()
    } else {
        format!("{prefix}:{key}")
    };
    Some((stored, value.trim().to_string()))
}

/// `<!-- key=value key=value -->` directives.
fn directive(line: &str) -> Option<Vec<(String, String)>> {
    let inner = line
        .trim()
        .strip_prefix("<!--")?
        .strip_suffix("-->")?
        .trim();
    let pairs: Option<Vec<_>> = inner
        .split_whitespace()
        .map(|token| {
            let (k, v) = token.split_once('=')?;
            (!k.is_empty() && !v.is_empty()).then(|| (k.to_string(), v.to_string()))
        })
        .collect();
    pairs.filter(|p| !p.is_empty())
}

/// An opening fence: `(backtick count, label)`. Unlabelled fences have `""`.
fn fence_open(line: &str) -> Option<(usize, &str)> {
    let ticks = line.len() - line.trim_start_matches('`').len();
    if ticks < 3 {
        return None;
    }
    let label = line[ticks..].trim();
    (!label.contains('`')).then_some((ticks, label))
}

fn fence_close(line: &str, ticks: usize) -> bool {
    let run = line.len() - line.trim_start_matches('`').len();
    run >= ticks && line[run..].trim().is_empty()
}

fn blocks(lines: &[&str], start: usize) -> Result<Vec<Block>> {
    let mut out = Vec::new();
    let mut i = start;
    while i < lines.len() {
        let Some((ticks, label)) = fence_open(lines[i]) else {
            i += 1;
            continue;
        };
        let (mut meta, directives) = pre_fence(lines, start, i);
        let line = i + 1;
        i += 1;
        let mut payload = Vec::new();
        let mut leading = true;
        loop {
            let Some(text) = lines.get(i) else {
                return Err(invalid(format!(
                    "fence opened at line {line} is never closed"
                )));
            };
            i += 1;
            if fence_close(text, ticks) {
                break;
            }
            if leading && let Some((key, value)) = annotation(text) {
                meta.entry(key).or_insert(value);
                continue;
            }
            leading = false;
            payload.push(*text);
        }
        out.push(Block {
            label: label.to_string(),
            meta,
            directives,
            payload: payload.join("\n"),
            line,
        });
    }
    Ok(out)
}

/// Annotations and directives on the lines just before a fence, allowing one
/// blank line between them and the fence, as the reference parser does.
type PreFence = (BTreeMap<String, String>, BTreeMap<String, String>);

fn pre_fence(lines: &[&str], floor: usize, fence: usize) -> PreFence {
    let mut meta = BTreeMap::new();
    let mut directives = BTreeMap::new();
    let mut j = fence;
    if j > floor && lines[j - 1].trim().is_empty() {
        j -= 1;
    }
    while j > floor {
        let text = lines[j - 1];
        if let Some((key, value)) = annotation(text) {
            // Walking upward: a higher line overwrites, so reading order wins.
            meta.insert(key, value);
        } else if let Some(pairs) = directive(text) {
            directives.extend(pairs);
        } else {
            break;
        }
        j -= 1;
    }
    (meta, directives)
}

/// Builds YAML frontmatter. Every string is written double-quoted, so no value
/// can change the document's structure.
#[derive(Debug, Default)]
pub struct Frontmatter {
    yaml: String,
}

impl Frontmatter {
    fn push(&mut self, indent: usize, item: bool, key: &str, value: Option<&str>) {
        self.yaml.push_str(&"  ".repeat(indent));
        if item {
            self.yaml.push_str("- ");
        }
        self.yaml.push_str(key);
        self.yaml.push(':');
        if let Some(value) = value {
            self.yaml.push(' ');
            self.yaml.push_str(value);
        }
        self.yaml.push('\n');
    }

    /// `key: "value"`.
    pub fn text(&mut self, indent: usize, key: &str, value: &str) -> &mut Self {
        self.push(indent, false, key, Some(&quote(value)));
        self
    }

    /// `key: 42`.
    pub fn number(&mut self, indent: usize, key: &str, value: u64) -> &mut Self {
        self.push(indent, false, key, Some(&value.to_string()));
        self
    }

    /// `key:` opening a nested mapping or sequence.
    pub fn open(&mut self, indent: usize, key: &str) -> &mut Self {
        self.push(indent, false, key, None);
        self
    }

    /// `- key: "value"`: the first field of a sequence entry.
    pub fn item(&mut self, indent: usize, key: &str, value: &str) -> &mut Self {
        self.push(indent, true, key, Some(&quote(value)));
        self
    }

    /// The finished `---` block.
    pub fn finish(&self) -> String {
        format!("---\n{}---\n", self.yaml)
    }
}

/// A YAML double-quoted scalar. JSON string escapes are a subset of YAML's.
fn quote(value: &str) -> String {
    serde_json::to_string(value).expect("a string always serializes")
}

/// One fenced block with its annotations before the fence. Refuses a payload
/// line that would close the fence early: the 1.5 reference parser recognises
/// only three-backtick fences, so a longer fence is not an escape.
pub fn fence(label: &str, annotations: &[(&str, &str)], payload: &str) -> Result<String> {
    if let Some(n) = payload
        .lines()
        .position(|l| l.trim_start().starts_with("```"))
    {
        return Err(invalid(format!(
            "{label} payload line {} starts with ```, which would end its fence",
            n + 1
        )));
    }
    let mut out = String::new();
    for (key, value) in annotations {
        if value.contains("-->") || value.contains('\n') {
            return Err(invalid(format!("annotation `{key}` cannot hold `{value}`")));
        }
        let key = if key.contains(':') {
            (*key).to_string()
        } else {
            format!("databook:{key}")
        };
        out.push_str(&format!("<!-- {key}: {value} -->\n"));
    }
    out.push_str(&format!("```{label}\n{}", payload.trim_end_matches('\n')));
    out.push_str("\n```\n");
    Ok(out)
}

#[cfg(test)]
#[path = "databook_tests.rs"]
mod tests;
