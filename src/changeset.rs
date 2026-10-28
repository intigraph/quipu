//! The store-wide change-set format, `quipu-changeset` v1 (aegis-4mk).
//!
//! One interchange format for every change Quipu exchanges: fork diffs,
//! transaction ranges, share deltas, and `POST /patch`. The body is
//! parent-bound SPARQL Update (ground `DELETE DATA` then ground `INSERT DATA`,
//! the share-delta syntax from `standard-share-artifact.md`). A `# key: value`
//! comment header carries base tx, valid time, source and shapes hash. RDF
//! Patch is the same change in another media type. Spec:
//! `docs/design/changeset-format.md`. The signed push-back envelope
//! (aegis-s24.4) wraps this format and signs [`ChangeSet::digest`].

use std::collections::BTreeSet;

use spargebra::GraphUpdateOperation;
use spargebra::term::GraphName;

use crate::error::{Error, Result};

/// Media type of the primary (SPARQL Update) form.
pub const SPARQL_UPDATE: &str = "application/sparql-update";
/// Media type of the RDF Patch form.
pub const RDF_PATCH: &str = "application/rdf-patch";
/// The only format version this build reads and writes.
pub const VERSION: &str = "v1";

/// One ground statement: a triple in canonical N-Triples term form (no
/// trailing ` .`) and an optional named graph IRI (`None` = default graph).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Statement {
    /// `<s> <p> <o>` or `<s> <p> "literal"`, as oxrdf's `Display` renders it.
    pub triple: String,
    /// Named graph IRI without angle brackets; `None` for the default graph.
    pub graph: Option<String>,
}

impl Statement {
    /// A statement from its rendered triple and graph.
    #[must_use]
    pub fn new(triple: impl Into<String>, graph: Option<String>) -> Self {
        Self {
            triple: triple.into(),
            graph,
        }
    }

    /// From an oxrdf quad, rendering terms canonically.
    #[must_use]
    pub fn from_quad(quad: &oxrdf::Quad) -> Self {
        let graph = match &quad.graph_name {
            oxrdf::GraphName::NamedNode(n) => Some(n.as_str().to_string()),
            _ => None,
        };
        Self::new(
            format!("{} {} {}", quad.subject, quad.predicate, quad.object),
            graph,
        )
    }

    /// The N-Quads line for this statement, without the trailing ` .`.
    #[must_use]
    pub fn nquad(&self) -> String {
        match &self.graph {
            Some(g) => format!("{} <{g}>", self.triple),
            None => self.triple.clone(),
        }
    }

    /// Parse back into an oxrdf quad.
    ///
    /// # Errors
    /// The rendered statement is not a valid N-Quads statement.
    pub fn to_quad(&self) -> Result<oxrdf::Quad> {
        let line = format!("{} .\n", self.nquad());
        let mut quads = oxrdfio::RdfParser::from_format(oxrdfio::RdfFormat::NQuads)
            .for_slice(line.as_bytes())
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| Error::InvalidValue(format!("changeset statement: {e}")))?;
        match (quads.pop(), quads.is_empty()) {
            (Some(q), true) => Ok(q),
            _ => Err(Error::InvalidValue(format!(
                "changeset statement is not one quad: {line}"
            ))),
        }
    }

    /// True when the subject or object is a blank node.
    #[must_use]
    pub fn has_blank_node(&self) -> bool {
        self.to_quad().is_ok_and(|q| {
            matches!(q.subject, oxrdf::NamedOrBlankNode::BlankNode(_))
                || matches!(q.object, oxrdf::Term::BlankNode(_))
        })
    }

    fn sparql_line(&self) -> String {
        match &self.graph {
            Some(g) => format!("  GRAPH <{g}> {{ {} . }}\n", self.triple),
            None => format!("  {} .\n", self.triple),
        }
    }
}

/// The change-set header. Every field but the version is optional.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub struct Header {
    /// Transaction whose state the deletes were read from.
    pub base_tx: Option<i64>,
    /// Transaction whose state the inserts reproduce, when recorded.
    pub target_tx: Option<i64>,
    /// Parent share id, when the pre-state is a share.
    pub base_share: Option<String>,
    /// `store_id` of the store the base was read from.
    pub store_id: Option<String>,
    /// ISO-8601 `valid_from` for inserted statements.
    pub valid_time: Option<String>,
    /// Provenance label for the applying transaction.
    pub source: Option<String>,
    /// SHA-256 of the shape sets at the pre-state (advisory).
    pub shapes_hash: Option<String>,
}

impl Header {
    fn pairs(&self) -> Vec<(&'static str, String)> {
        let mut out = vec![("quipu-changeset", VERSION.to_string())];
        let mut put = |key, value: Option<String>| {
            if let Some(v) = value {
                out.push((key, v));
            }
        };
        put("base-tx", self.base_tx.map(|t| t.to_string()));
        put("target-tx", self.target_tx.map(|t| t.to_string()));
        put("base-share", self.base_share.clone());
        put("store-id", self.store_id.clone());
        put("valid-time", self.valid_time.clone());
        put("source", self.source.clone());
        put("shapes-hash", self.shapes_hash.clone());
        out
    }

    /// Apply one `key: value` pair. Returns the declared digest, if this was it.
    fn set(&mut self, key: &str, value: &str) -> Result<Option<String>> {
        let tx = |v: &str| {
            v.parse::<i64>()
                .map_err(|_| Error::InvalidValue(format!("changeset header {key}: not a tx: {v}")))
        };
        match key {
            "quipu-changeset" if value != VERSION => {
                return Err(Error::InvalidValue(format!(
                    "unsupported changeset version {value}; this build reads {VERSION}"
                )));
            }
            "base-tx" => self.base_tx = Some(tx(value)?),
            "target-tx" => self.target_tx = Some(tx(value)?),
            "base-share" => self.base_share = Some(value.into()),
            "store-id" => self.store_id = Some(value.into()),
            "valid-time" => {
                let normalized = crate::time::normalize_rfc3339_utc(value).ok_or_else(|| {
                    Error::InvalidValue(format!(
                        "changeset header valid-time is not an RFC 3339 timestamp: {value}"
                    ))
                })?;
                self.valid_time = Some(normalized);
            }
            "source" => self.source = Some(value.into()),
            "shapes-hash" => self.shapes_hash = Some(value.into()),
            "digest" => return Ok(Some(value.into())),
            // Unknown keys are ignored so later versions can add fields.
            _ => {}
        }
        Ok(None)
    }
}

/// A change-set: header, deletes, inserts. Statements are sorted and unique.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeSet {
    /// Base, valid time, source and shapes hash.
    pub header: Header,
    /// Statements removed, applied first.
    pub deletes: Vec<Statement>,
    /// Statements added.
    pub inserts: Vec<Statement>,
}

impl ChangeSet {
    /// Build a change-set, normalizing order and refusing contradictions.
    ///
    /// # Errors
    /// A statement is both deleted and inserted.
    pub fn new(header: Header, deletes: Vec<Statement>, inserts: Vec<Statement>) -> Result<Self> {
        let deletes: BTreeSet<_> = deletes.into_iter().collect();
        let inserts: BTreeSet<_> = inserts.into_iter().collect();
        if let Some(both) = deletes.intersection(&inserts).next() {
            return Err(Error::InvalidValue(format!(
                "changeset statement is both deleted and inserted: {}",
                both.nquad()
            )));
        }
        Ok(Self {
            header,
            deletes: deletes.into_iter().collect(),
            inserts: inserts.into_iter().collect(),
        })
    }

    /// The change that turns `before` into `after`.
    pub fn from_states(
        header: Header,
        before: impl IntoIterator<Item = Statement>,
        after: impl IntoIterator<Item = Statement>,
    ) -> Self {
        let before: BTreeSet<_> = before.into_iter().collect();
        let after: BTreeSet<_> = after.into_iter().collect();
        Self {
            header,
            deletes: before.difference(&after).cloned().collect(),
            inserts: after.difference(&before).cloned().collect(),
        }
    }

    /// No statements on either side.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.deletes.is_empty() && self.inserts.is_empty()
    }

    fn patch_body(&self) -> String {
        let mut out = String::new();
        for (row, side) in [("D", &self.deletes), ("A", &self.inserts)] {
            for s in side {
                out.push_str(&format!("{row} {} .\n", s.nquad()));
            }
        }
        out
    }

    /// SHA-256 as bare lowercase hex (no `sha256:` prefix) over the RDF Patch `D`/`A` lines: statements only, never
    /// the header. This is the `delta_sha256` the s24.4 envelope signs.
    #[must_use]
    pub fn digest(&self) -> String {
        let prefixed = crate::share::sha256(self.patch_body().as_bytes());
        // `share::sha256` returns `sha256:<hex>`; the digest is the bare hex and
        // the wire form adds the prefix exactly once.
        prefixed
            .strip_prefix("sha256:")
            .unwrap_or(&prefixed)
            .to_string()
    }

    /// The SPARQL Update body alone, no header: what any SPARQL 1.1 endpoint
    /// can apply. Empty when the change-set is empty.
    #[must_use]
    pub fn sparql_body(&self) -> String {
        let mut out = String::new();
        for (op, side) in [
            ("DELETE DATA", &self.deletes),
            ("INSERT DATA", &self.inserts),
        ] {
            if side.is_empty() {
                continue;
            }
            out.push_str(op);
            out.push_str(" {\n");
            for s in side {
                out.push_str(&s.sparql_line());
            }
            out.push_str("};\n");
        }
        out
    }

    /// The header as `# key: value` comment lines, digest last.
    #[must_use]
    pub fn sparql_header(&self) -> String {
        let mut out = String::new();
        for (k, v) in self.header.pairs() {
            out.push_str(&format!("# {k}: {v}\n"));
        }
        out.push_str(&format!("# digest: sha256:{}\n", self.digest()));
        out
    }

    /// The primary form: comment header then the SPARQL Update body.
    #[must_use]
    pub fn to_sparql_update(&self) -> String {
        format!("{}{}", self.sparql_header(), self.sparql_body())
    }

    /// The RDF Patch form: `H` rows, then one `TX`/`TC` block.
    #[must_use]
    pub fn to_rdf_patch(&self) -> String {
        let lit = |v: &str| oxrdf::Literal::new_simple_literal(v).to_string();
        let mut out = String::new();
        for (k, v) in self.header.pairs() {
            out.push_str(&format!("H {k} {} .\n", lit(&v)));
        }
        out.push_str(&format!(
            "H digest {} .\n",
            lit(&format!("sha256:{}", self.digest()))
        ));
        out.push_str("TX .\n");
        out.push_str(&self.patch_body());
        out.push_str("TC .\n");
        out
    }

    /// Render in `media_type` ([`SPARQL_UPDATE`] or [`RDF_PATCH`]).
    #[must_use]
    pub fn render(&self, media_type: &str) -> String {
        if media_type == RDF_PATCH {
            self.to_rdf_patch()
        } else {
            self.to_sparql_update()
        }
    }

    /// Parse either form. A declared `digest` must match.
    ///
    /// # Errors
    /// Unsupported media type or version, an unrestricted update, a malformed
    /// row, a contradiction, or a digest mismatch.
    pub fn parse(text: &str, media_type: &str) -> Result<Self> {
        let (cs, declared) = match media_type {
            SPARQL_UPDATE => parse_sparql(text)?,
            RDF_PATCH => parse_patch(text)?,
            other => {
                return Err(Error::InvalidValue(format!(
                    "unsupported changeset media type {other}; use {SPARQL_UPDATE} or {RDF_PATCH}"
                )));
            }
        };
        if let Some(declared) = declared {
            let digest = cs.digest();
            if declared.strip_prefix("sha256:") != Some(digest.as_str()) {
                return Err(Error::InvalidValue(format!(
                    "changeset digest mismatch: declared {declared}, computed sha256:{digest}"
                )));
            }
        }
        Ok(cs)
    }
}

fn parse_sparql(text: &str) -> Result<(ChangeSet, Option<String>)> {
    let mut header = Header::default();
    let mut declared = None;
    for line in text.lines().map(str::trim) {
        if line.is_empty() {
            continue;
        }
        let Some(comment) = line.strip_prefix('#') else {
            break;
        };
        if let Some((k, v)) = comment.split_once(':') {
            let digest = header.set(k.trim(), v.trim())?;
            declared = declared.or(digest);
        }
    }
    let (mut deletes, mut inserts) = (Vec::new(), Vec::new());
    if text
        .lines()
        .any(|l| !l.trim().is_empty() && !l.trim().starts_with('#'))
    {
        crate::sparql_structure::check(text)?;
        let update =
            crate::sparql_structure::parse_update(spargebra::SparqlParser::new(), text)?
                .map_err(|e| Error::InvalidValue(format!("changeset is not SPARQL Update: {e}")))?;
        for op in update.operations {
            match op {
                GraphUpdateOperation::DeleteData { data } if inserts.is_empty() => {
                    for q in data {
                        deletes.push(Statement::new(
                            format!("{} {} {}", q.subject, q.predicate, q.object),
                            graph_iri(&q.graph_name),
                        ));
                    }
                }
                GraphUpdateOperation::InsertData { data } => {
                    for q in data {
                        inserts.push(Statement::new(
                            format!("{} {} {}", q.subject, q.predicate, q.object),
                            graph_iri(&q.graph_name),
                        ));
                    }
                }
                GraphUpdateOperation::DeleteData { .. } => {
                    return Err(Error::InvalidValue(
                        "changeset DELETE DATA must precede INSERT DATA".into(),
                    ));
                }
                _ => {
                    return Err(Error::InvalidValue(
                        "changeset allows only ground DELETE DATA and INSERT DATA".into(),
                    ));
                }
            }
        }
    }
    Ok((ChangeSet::new(header, deletes, inserts)?, declared))
}

fn graph_iri(graph: &GraphName) -> Option<String> {
    match graph {
        GraphName::NamedNode(n) => Some(n.as_str().to_string()),
        GraphName::DefaultGraph => None,
    }
}

fn parse_patch(text: &str) -> Result<(ChangeSet, Option<String>)> {
    let mut header = Header::default();
    let mut declared = None;
    let (mut deletes, mut inserts) = (Vec::new(), Vec::new());
    for line in text.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (row, rest) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
        let rest = rest.trim();
        match row {
            "TX" | "TC" => {}
            "H" => {
                let (key, value) = rest
                    .split_once(char::is_whitespace)
                    .ok_or_else(|| Error::InvalidValue(format!("RDF Patch header row: {line}")))?;
                let value = header_value(value.trim())?;
                let digest = header.set(key, &value)?;
                declared = declared.or(digest);
            }
            "D" | "A" => {
                let parsed = oxrdfio::RdfParser::from_format(oxrdfio::RdfFormat::NQuads)
                    .for_slice(format!("{rest}\n").as_bytes())
                    .collect::<std::result::Result<Vec<_>, _>>()
                    .map_err(|e| Error::InvalidValue(format!("RDF Patch row {line}: {e}")))?;
                let [q] = parsed.as_slice() else {
                    return Err(Error::InvalidValue(format!("RDF Patch row: {line}")));
                };
                let s = Statement::from_quad(q);
                if row == "D" {
                    if !inserts.is_empty() {
                        return Err(Error::InvalidValue(
                            "changeset D rows must precede A rows".into(),
                        ));
                    }
                    deletes.push(s);
                } else {
                    inserts.push(s);
                }
            }
            _ => {
                return Err(Error::InvalidValue(format!(
                    "unsupported RDF Patch row {row}: {line}"
                )));
            }
        }
    }
    Ok((ChangeSet::new(header, deletes, inserts)?, declared))
}

/// An `H` row's value term (`"literal" .` or `<iri> .`), as a plain string.
fn header_value(term: &str) -> Result<String> {
    let doc = format!("<urn:h> <urn:h> {term}\n");
    let triples = oxrdfio::RdfParser::from_format(oxrdfio::RdfFormat::NTriples)
        .for_slice(doc.as_bytes())
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| Error::InvalidValue(format!("RDF Patch header value {term}: {e}")))?;
    match triples.first().map(|t| &t.object) {
        Some(oxrdf::Term::Literal(l)) => Ok(l.value().to_string()),
        Some(oxrdf::Term::NamedNode(n)) => Ok(n.as_str().to_string()),
        _ => Err(Error::InvalidValue(format!(
            "RDF Patch header value: {term}"
        ))),
    }
}

/// Pick the response media type from an `Accept` header: RDF Patch when it
/// outranks SPARQL Update, else SPARQL Update (also the default).
#[must_use]
pub fn negotiate(accept: Option<&str>) -> &'static str {
    let Some(accept) = accept else {
        return SPARQL_UPDATE;
    };
    let (mut sparql, mut patch) = (-1.0_f32, -1.0_f32);
    for range in accept.split(',') {
        let mut parts = range.split(';').map(str::trim);
        let kind = parts.next().unwrap_or("").to_ascii_lowercase();
        let q = parts
            .find_map(|p| p.strip_prefix("q="))
            .and_then(|v| v.parse::<f32>().ok())
            .unwrap_or(1.0);
        match kind.as_str() {
            RDF_PATCH => patch = patch.max(q),
            SPARQL_UPDATE | "*/*" | "application/*" => sparql = sparql.max(q),
            _ => {}
        }
    }
    if patch > sparql {
        RDF_PATCH
    } else {
        SPARQL_UPDATE
    }
}

#[cfg(test)]
#[path = "changeset_tests.rs"]
mod tests;
