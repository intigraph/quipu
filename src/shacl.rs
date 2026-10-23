//! SHACL write-time validation via rudof.
//!
//! Loads SHACL shapes from Turtle and validates proposed RDF data against them
//! before allowing it into the fact log. Returns structured feedback on failures
//! so agents can fix and retry.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::io::Read;
use std::sync::{Arc, Mutex, OnceLock};

use rudof_rdf::rdf_core::RDFFormat;
use rudof_rdf::rdf_core::term::Object;
use rudof_rdf::rdf_impl::{OxigraphInMemory, ReaderMode};
use shacl_engine::ir::IRSchema;
use shacl_engine::validator::ShaclValidationMode;
use shacl_engine::validator::processor::ShaclProcessor;
use shacl_engine::validator::store::ShaclDataManager;

#[path = "shacl_targets.rs"]
mod targets;

#[path = "shacl_write_paths.rs"]
pub(crate) mod write_paths;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::resolution::EntityCandidate;

fn report_term(term: &Object) -> String {
    match term {
        Object::Literal(literal) => literal.lexical_form(),
        _ => term.to_string(),
    }
}

fn single_shape_message(shapes: &str) -> Option<String> {
    let pattern = regex::Regex::new(r#"sh:message\s+(\"(?:\\.|[^\"])*\")"#).ok()?;
    let mut matches = pattern.captures_iter(shapes);
    let first = matches.next()?.get(1)?.as_str();
    if matches.next().is_some() {
        return None;
    }
    serde_json::from_str(first).ok()
}

fn report_severity(severity: &shacl_engine::types::Severity) -> String {
    match severity {
        shacl_engine::types::Severity::Generic(iri) => iri.to_string(),
        other => other.to_string(),
    }
}

/// Structured feedback from SHACL validation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationFeedback {
    /// Whether the data conforms to the shapes, in the sense of SHACL's
    /// `sh:conforms`: true only when there are NO results of ANY severity, so a
    /// lone `sh:Warning` makes it false. The W3C suite pins this. For "does
    /// anything block", ask [`ValidationFeedback::blocks`] instead.
    /// (`shacl_context`'s store-context repair reports `violations == 0` here.)
    pub conforms: bool,
    /// Number of violations found.
    pub violations: usize,
    /// Number of warnings found.
    pub warnings: usize,
    /// Individual violation/warning details.
    pub results: Vec<ValidationIssue>,
    /// Entity resolution candidates — present when resolution is enabled and
    /// near-duplicate entities were detected during write.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolution_candidates: Option<Vec<EntityCandidate>>,
}

impl ValidationFeedback {
    /// Whether any result has `sh:Violation` severity. Narrower than
    /// `!conforms`: `sh:Warning` and `sh:Info` results are reported but do not
    /// block. Share import and compose gate on this (aegis-1mv0to), so a shape
    /// can declare itself advisory with `sh:severity sh:Warning`.
    #[must_use]
    pub fn blocks(&self) -> bool {
        self.violations > 0
    }
}

/// A single validation issue.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationIssue {
    /// Severity: "violation", "warning", or "info".
    pub severity: String,
    /// The focus node that failed validation.
    pub focus_node: String,
    /// The SHACL component that triggered the issue.
    pub component: String,
    /// The property path involved (if any).
    pub path: Option<String>,
    /// The offending value (if any).
    pub value: Option<String>,
    /// The source shape (if any).
    pub source_shape: Option<String>,
    /// Human-readable message.
    pub message: Option<String>,
}

/// A SHACL validator that holds loaded shapes and validates data against them.
///
/// The parsed shapes graph is retained (behind a `Mutex`, because rudof needs
/// `&mut` to validate) and REUSED across `validate` calls. Previously this type
/// kept only the shapes *string*: `from_turtle` parsed the shapes and
/// threw the parse away, then every `validate` re-parsed them. With the real
/// aegis shape sets (86 KB) that was ~19 ms of parsing done twice on every
/// single write, and it dominated validation cost — see `examples/shacl_cost.rs`.
pub struct Validator {
    shapes_turtle: String,
    /// Compiled shapes schema, reused across validation calls.
    schema: Mutex<IRSchema>,
}

impl Validator {
    /// Create a new validator from SHACL shapes in Turtle format.
    pub fn from_turtle(shapes: &str) -> Result<Self> {
        let mut reader = std::io::BufReader::new(shapes.as_bytes());
        let schema = ShaclDataManager::load(
            &mut reader,
            "shapes",
            &RDFFormat::Turtle,
            Some("http://quipu.local/shapes"),
        )
        .map_err(|e| Error::InvalidValue(format!("SHACL parse error: {e}")))?;
        Ok(Self {
            shapes_turtle: shapes.to_string(),
            schema: Mutex::new(schema),
        })
    }

    /// The shapes this validator was built from.
    #[must_use]
    pub fn shapes_turtle(&self) -> &str {
        &self.shapes_turtle
    }

    /// Load shapes from a reader.
    pub fn from_reader(mut reader: impl Read) -> Result<Self> {
        let mut shapes = String::new();
        reader
            .read_to_string(&mut shapes)
            .map_err(|e| Error::InvalidValue(format!("read error: {e}")))?;
        Self::from_turtle(&shapes)
    }

    /// Validate RDF data (as Turtle bytes) against the loaded shapes.
    ///
    /// Returns structured feedback. If `conforms` is true, the data is valid.
    pub fn validate(&self, data: &[u8]) -> Result<ValidationFeedback> {
        // Reuse the already-parsed shapes graph. `read_data` below is called
        // with merge=None (=false), which replaces the RDF data outright, so a
        // reused instance carries no data from a previous validation.
        let schema = self
            .schema
            .lock()
            .map_err(|e| Error::InvalidValue(format!("shapes lock poisoned: {e}")))?;

        let mut data_reader = std::io::BufReader::new(data);
        let data = OxigraphInMemory::from_reader(
            &mut data_reader,
            "data",
            &RDFFormat::Turtle,
            Some("http://quipu.local/data"),
            &ReaderMode::Lax,
        )
        .map_err(|e| Error::InvalidValue(format!("data load error: {e}")))?;

        let mut validator = targets::SubclassValidation(data);
        let report = ShaclProcessor::validate(
            &mut validator,
            &schema,
            &ShaclValidationMode::Native,
            &shacl_engine::validator::ShaclConfig::default(),
        )
        .map_err(|e| Error::InvalidValue(format!("SHACL validation error: {e}")))?;

        let mut issues = Vec::new();
        let shape_message = single_shape_message(&self.shapes_turtle);
        for result in report.results() {
            let focus_node = report_term(result.focus_node());
            let component = report_term(result.constraint_component());
            let value = result.value().map(report_term).or_else(|| {
                component
                    .ends_with("#HasValueConstraintComponent")
                    .then(|| focus_node.clone())
            });
            issues.push(ValidationIssue {
                severity: report_severity(result.severity()),
                focus_node,
                component,
                path: result.path().map(|p| format!("{p}")),
                value,
                source_shape: result.source().map(report_term),
                message: shape_message
                    .clone()
                    .or_else(|| result.message().get(None).cloned()),
            });
        }

        Ok(ValidationFeedback {
            conforms: report.conforms(),
            violations: report.get_count_of(&shacl_engine::types::Severity::Violation),
            warnings: report.get_count_of(&shacl_engine::types::Severity::Warning),
            results: issues,
            resolution_candidates: None,
        })
    }

    /// Validate proposed Turtle data and return Ok(()) if valid, or Err with
    /// the first violation message if not.
    pub fn validate_or_reject(&self, data: &[u8]) -> Result<()> {
        let feedback = self.validate(data)?;
        if feedback.conforms {
            Ok(())
        } else {
            let msg = feedback.results.first().map_or_else(
                || "SHACL validation failed".to_string(),
                |r| {
                    format!(
                        "SHACL violation on {}: {} (component: {}{})",
                        r.focus_node,
                        r.message.as_deref().unwrap_or("constraint violated"),
                        r.component,
                        r.path
                            .as_ref()
                            .map(|p| format!(", path: {p}"))
                            .unwrap_or_default()
                    )
                },
            );
            Err(Error::InvalidValue(format!(
                "{msg}. Hint: propose a schema change via quipu_propose_schema_change"
            )))
        }
    }
}

/// Process-wide cache of parsed shape graphs, keyed by a hash of the shapes
/// Turtle. The stored shapes change rarely (a `/shapes` load) while writes are
/// continuous, so without this every write re-parses the same 86 KB of shapes.
///
/// Bounded: the key space is "distinct shape sets in use", normally 1. The cap
/// exists so a caller passing per-request inline shapes (`/knot` accepts them)
/// cannot grow this without limit.
type ValidatorCache = Mutex<HashMap<u64, Arc<Validator>>>;

const VALIDATOR_CACHE_CAP: usize = 16;

fn validator_cache() -> &'static ValidatorCache {
    static CACHE: OnceLock<ValidatorCache> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn shapes_key(shapes_turtle: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    shapes_turtle.hash(&mut hasher);
    hasher.finish()
}

/// Get a validator for these shapes, parsing them only on a cache miss.
///
/// On a hash collision between two *different* shape sets the cached validator
/// would be wrong, so the stored shapes are compared before reuse.
pub fn cached_validator(shapes_turtle: &str) -> Result<Arc<Validator>> {
    cached_validator_in(validator_cache(), shapes_turtle)
}

/// The lookup behind `cached_validator`, with the cache as a parameter so
/// tests can exercise the caching contract against a private cache instead of
/// the process-global one (which parallel tests evict from at will).
fn cached_validator_in(cache: &ValidatorCache, shapes_turtle: &str) -> Result<Arc<Validator>> {
    let key = shapes_key(shapes_turtle);
    {
        let cache = cache
            .lock()
            .map_err(|e| Error::InvalidValue(format!("validator cache poisoned: {e}")))?;
        if let Some(v) = cache.get(&key)
            && v.shapes_turtle() == shapes_turtle
        {
            return Ok(Arc::clone(v));
        }
    }

    // Parse outside the lock: it is the expensive step, and holding the cache
    // lock across it would serialise every concurrent write behind one parse.
    let validator = Arc::new(Validator::from_turtle(shapes_turtle)?);

    let mut cache = cache
        .lock()
        .map_err(|e| Error::InvalidValue(format!("validator cache poisoned: {e}")))?;
    if cache.len() >= VALIDATOR_CACHE_CAP {
        cache.clear();
    }
    cache.insert(key, Arc::clone(&validator));
    Ok(validator)
}

/// Convenience: validate proposed data against shapes, both as Turtle strings.
///
/// Returns structured feedback for agent consumption. The parsed shapes graph
/// is cached across calls — see `cached_validator`.
pub fn validate_shapes(shapes_turtle: &str, data_turtle: &str) -> Result<ValidationFeedback> {
    let validator = cached_validator(shapes_turtle)?;
    validator.validate(data_turtle.as_bytes())
}

#[cfg(test)]
#[path = "shacl_tests.rs"]
mod tests;

/// The result of routing a combined shapes document by `quipu:onViolation`.
///
/// Event-based P3 (design §5/§7): a shape annotated `quipu:onViolation
/// "emit"` observes violations as `shacl.violation` events WITHOUT gating the
/// write; every other shape (unannotated, or annotated `"reject"`) keeps the
/// hard-gate semantics. DEFAULT REJECT is decided (design §9.3) — a shape opts
/// INTO emit, never out of reject by omission.
#[derive(Debug, Clone)]
pub struct ShapesSplit {
    /// Shapes that gate the transaction (hard reject on violation).
    pub reject: String,
    /// Shapes whose violations become events; empty when nothing is annotated.
    pub emit: String,
    /// Whether any emit-annotated shape was found (spares a validator run).
    pub has_emit: bool,
}

/// Split a Turtle shapes document into reject-mode and emit-mode documents by
/// the `quipu:onViolation` annotation on each top-level shape block.
///
/// Two steps with different jobs:
/// - BOUNDARIES are textual: a block is a top-level statement of the house
///   style `<subject> a sh:NodeShape ; … .`, ending with `.` at bracket depth 0
///   at end-of-line. The lexer knows literals, IRIs and comments, so a `#`,
///   bracket or `.` inside a string is data.
/// - MEMBERSHIP is parsed RDF (aegis-g51c C7): the block, with the prefixes
///   declared before it, is parsed as Turtle, and it routes to emit only if
///   every top-level subject (one that is not an object inside the block)
///   carries exactly the policy `quipu:onViolation "emit"`. Text never decides,
///   so the annotation quoted inside a `sh:message`, written in a comment, or
///   set on a nested property shape cannot move a rejecting shape.
///
/// Everything uncertain fails closed to REJECT: a block that does not parse,
/// a conflicting or non-string policy, and unterminated trailing content.
/// Prefix lines are replicated into both documents so each stands alone.
pub fn split_shapes_by_policy(shapes_turtle: &str) -> ShapesSplit {
    let mut prefixes = String::new();
    // (block text, the prefixes declared before it)
    let mut blocks: Vec<(String, String)> = Vec::new();
    let mut current = String::new();
    let mut lexer = TurtleLexer::default();

    for line in shapes_turtle.lines() {
        let trimmed = line.trim();
        if !lexer.in_literal()
            && (trimmed.starts_with("@prefix") || trimmed.to_lowercase().starts_with("prefix "))
        {
            prefixes.push_str(line);
            prefixes.push('\n');
            continue;
        }
        // Comments and blank lines between blocks belong to no block.
        if current.is_empty()
            && !lexer.in_literal()
            && (trimmed.is_empty() || trimmed.starts_with('#'))
        {
            continue;
        }
        current.push_str(line);
        current.push('\n');
        let code = lexer.scan_line(line);
        if lexer.depth == 0 && !lexer.in_literal() && code.trim_end().ends_with('.') {
            blocks.push((std::mem::take(&mut current), prefixes.clone()));
        }
    }

    let mut reject = prefixes.clone();
    let mut emit = prefixes;
    let mut has_emit = false;
    for (block, declared) in blocks {
        if block_is_emit(&declared, &block) {
            emit.push_str(&block);
            emit.push('\n');
            has_emit = true;
        } else {
            reject.push_str(&block);
            reject.push('\n');
        }
    }
    // Unterminated trailing content (unbalanced brackets, an open literal, a
    // missing final '.') ALWAYS rejects, whatever annotation it contains: the
    // splitter could not tell where its shapes begin and end (aegis-g51c).
    if !current.trim().is_empty() {
        reject.push_str(&current);
        reject.push('\n');
    }
    ShapesSplit {
        reject,
        emit,
        has_emit,
    }
}

/// The policy predicate, under both IRIs the admission path accepts.
const ON_VIOLATION_IRIS: [&str; 2] = [
    "http://quipu.dev/ontology/onViolation",
    "http://quipu.dev/ns#onViolation",
];

/// Whether one block, parsed with the prefixes in force at its position, is
/// entirely emit: every top-level subject, every declared `sh:NodeShape` and
/// every targeted shape has the single policy `"emit"`. A shape another block
/// member references (`sh:node`) is an object, not a root; without the last two
/// it rode along with its emit referrer and stopped rejecting (ian, #726).
fn block_is_emit(prefixes: &str, block: &str) -> bool {
    use oxrdf::Term;
    let document = format!("{prefixes}{block}");
    let Ok(triples) = oxttl::TurtleParser::new()
        .for_reader(document.as_bytes())
        .collect::<std::result::Result<Vec<_>, _>>()
    else {
        return false;
    };
    let objects: std::collections::HashSet<Term> =
        triples.iter().map(|t| t.object.clone()).collect();
    const SH: &str = "http://www.w3.org/ns/shacl#";
    let declared_shape = |t: &oxrdf::Triple| {
        let predicate = t.predicate.as_str();
        (predicate == oxrdf::vocab::rdf::TYPE.as_str()
            && matches!(&t.object, Term::NamedNode(n) if n.as_str() == format!("{SH}NodeShape")))
            || predicate
                .strip_prefix(SH)
                .is_some_and(|local| local.starts_with("target"))
    };
    let roots: std::collections::BTreeSet<String> = triples
        .iter()
        .filter(|t| !objects.contains(&Term::from(t.subject.clone())) || declared_shape(t))
        .map(|t| t.subject.to_string())
        .collect();
    if roots.is_empty() {
        return false;
    }
    let mut policies: HashMap<String, Vec<Option<&str>>> = HashMap::new();
    for t in &triples {
        if !ON_VIOLATION_IRIS.contains(&t.predicate.as_str()) {
            continue;
        }
        let value = match &t.object {
            // A plain string only: "emit"@en or a typed value is not the policy.
            Term::Literal(l)
                if l.language().is_none() && l.datatype() == oxrdf::vocab::xsd::STRING =>
            {
                Some(l.value())
            }
            _ => None,
        };
        policies
            .entry(t.subject.to_string())
            .or_default()
            .push(value);
    }
    roots.iter().all(|root| {
        policies
            .get(root)
            .is_some_and(|values| values.iter().all(|v| *v == Some("emit")))
    })
}

/// Just enough Turtle lexing to find statement boundaries: string literals
/// (short and long, single and double quoted, with escapes), IRIs and comments.
/// A `#`, bracket or `.` inside a literal or IRI is data, not syntax. Before
/// aegis-g51c the splitter cut every line at its first `#`, so a `sh:pattern`
/// containing `#` (camayoc-core) lost its closing brackets and the rest of the
/// combined document collapsed into one block that routed as a whole.
#[derive(Default)]
struct TurtleLexer {
    depth: i32,
    /// Open literal: (quote char, long form).
    literal: Option<(char, bool)>,
}

impl TurtleLexer {
    fn in_literal(&self) -> bool {
        self.literal.is_some()
    }

    /// Scan one line, updating bracket depth and literal state; return the
    /// line with any comment removed and every literal's CONTENT masked. Only
    /// statement boundaries are read from it; routing is decided by parsing.
    fn scan_line(&mut self, line: &str) -> String {
        let chars: Vec<char> = line.chars().collect();
        let mut code = String::with_capacity(line.len());
        // Content of an open SHORT literal on this line (it cannot span lines).
        let mut short = String::new();
        let mask = |text: &str| -> String { "_".repeat(text.chars().count()) };
        let mut i = 0;
        while i < chars.len() {
            let ch = chars[i];
            if let Some((quote, long)) = self.literal {
                if ch == '\\' {
                    let escaped = if chars.get(i + 1).is_some() {
                        "__"
                    } else {
                        "_"
                    };
                    if long {
                        code.push_str(escaped);
                    } else {
                        short.push_str(escaped);
                    }
                    i += 2;
                    continue;
                }
                if ch == quote && !long {
                    code.push_str(&mask(&std::mem::take(&mut short)));
                    code.push(quote);
                    self.literal = None;
                    i += 1;
                    continue;
                }
                if ch == quote
                    && chars.get(i + 1) == Some(&quote)
                    && chars.get(i + 2) == Some(&quote)
                {
                    code.extend([quote; 3]);
                    i += 3;
                    self.literal = None;
                    continue;
                }
                if long {
                    code.push('_');
                } else {
                    short.push(ch);
                }
                i += 1;
                continue;
            }
            match ch {
                '#' => break,
                '"' | '\'' => {
                    let long = chars.get(i + 1) == Some(&ch) && chars.get(i + 2) == Some(&ch);
                    if long {
                        code.push(ch);
                        code.push(ch);
                        code.push(ch);
                        i += 3;
                    } else {
                        code.push(ch);
                        i += 1;
                    }
                    self.literal = Some((ch, long));
                    continue;
                }
                '<' => {
                    // An IRI reference: copy through '>' verbatim.
                    let close = chars[i..].iter().position(|&c| c == '>');
                    let stop = close.map_or(chars.len(), |p| i + p + 1);
                    code.extend(&chars[i..stop]);
                    i = stop;
                    continue;
                }
                '[' | '(' => self.depth += 1,
                ']' | ')' => self.depth -= 1,
                _ => {}
            }
            code.push(ch);
            i += 1;
        }
        // A short literal cannot span lines; leaving one open would swallow the
        // rest of the document, so it stays open and the tail fails closed.
        code.push_str(&mask(&short));
        code
    }
}

#[cfg(all(test, feature = "shacl"))]
#[path = "shacl_split_tests.rs"]
mod shacl_split_tests;

#[cfg(test)]
mod split_tests {
    use super::*;

    const SHAPES: &str = r#"@prefix sh: <http://www.w3.org/ns/shacl#> .
@prefix aegis: <http://aegis.gastown.local/ontology/> .
@prefix quipu: <http://quipu.dev/ontology/> .
@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .

aegis:HardShape a sh:NodeShape ;
    sh:targetClass aegis:Thing ;
    sh:property [ sh:path rdfs:label ; sh:minCount 1 ] .

aegis:SoftShape a sh:NodeShape ;
    quipu:onViolation "emit" ;
    sh:targetClass aegis:Widget ;
    sh:property [ sh:path aegis:size ; sh:minCount 1 ] .
"#;

    #[test]
    fn routes_emit_annotated_block_and_keeps_default_reject() {
        let split = split_shapes_by_policy(SHAPES);
        assert!(split.has_emit);
        assert!(split.reject.contains("HardShape"));
        assert!(!split.reject.contains("SoftShape"));
        assert!(split.emit.contains("SoftShape"));
        assert!(!split.emit.contains("HardShape"));
        // Both documents carry the prefixes and PARSE as shapes.
        for doc in [&split.reject, &split.emit] {
            assert!(doc.contains("@prefix sh:"));
            validate_shapes(doc, "@prefix ex: <http://example.org/> .\n").unwrap();
        }
    }

    #[test]
    fn explicit_reject_annotation_stays_reject() {
        let shapes = SHAPES.replace("\"emit\"", "\"reject\"");
        let split = split_shapes_by_policy(&shapes);
        assert!(!split.has_emit);
        assert!(split.reject.contains("SoftShape"));
    }

    #[test]
    fn unannotated_document_is_pure_reject_and_byte_stable_semantics() {
        let shapes = SHAPES
            .lines()
            .filter(|l| !l.contains("quipu:onViolation"))
            .collect::<Vec<_>>()
            .join("\n");
        let split = split_shapes_by_policy(&shapes);
        assert!(!split.has_emit);
        assert!(split.reject.contains("HardShape") && split.reject.contains("SoftShape"));
        assert!(split.emit.trim_end().ends_with('.') || !split.emit.contains("Shape"));
    }
}

#[cfg(test)]
#[path = "code_entity_tests.rs"]
mod code_entity_tests;

#[cfg(test)]
#[path = "governance_tests.rs"]
mod governance_tests;
