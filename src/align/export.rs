//! Export a mapping set as standard SSSOM: SSSOM/TSV and SSSOM/RDF (Turtle).
//!
//! The working TSV that `propose`, `decide` and `apply` pass between them is
//! SSSOM-shaped, but it is quipu's own file: it carries full IRIs, no
//! `license`, and its exact bytes are the `sha256` that `apply` guards on, so it
//! cannot change shape without breaking every set an operator is part-way
//! through deciding. This module is the separate, read-only step that turns
//! such a set into one another SSSOM tool can load.
//!
//! Spec: <https://mapping-commons.github.io/sssom/> (data model, SSSOM/TSV,
//! SSSOM/RDF). The rules this module enforces, and why each one refuses rather
//! than guesses:
//!
//! * **Identifiers are CURIEs in TSV**, and every non-built-in prefix MUST be
//!   declared. An IRI no declared prefix covers is refused: inventing a prefix
//!   would publish an identifier nobody governs.
//! * **`license` is required** on the set. There is no default, because a
//!   licence quipu picked is a licence nobody granted.
//! * **`author_id` is an entity reference.** quipu records the reviewer as a
//!   bare name; export maps it into a namespace only when the operator names one
//!   (`author_prefix`). It never adds an author to a row that has none: an
//!   unauthored row is a candidate, and a declined row asserts nothing.
//! * **Output is deterministic**: rows in the set's canonical order, a fixed
//!   column and key order, and blank nodes labelled by position.
//!
//! The Turtle is parsed back before it is returned, so a writer bug surfaces
//! as an error here and never as a file a consumer cannot read.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use crate::error::{Error, Result};
use crate::namespace::QUIPU;

use super::sssom::{Mapping, MappingSet, SEMAPV_NS, SSSOM_NS, is_xsd_date};

/// The SSSOM built-in prefixes. They MAY be omitted from a `curie_map`, MUST
/// mean exactly this if declared, and a canonical writer leaves them out.
pub const BUILT_IN_PREFIXES: [(&str, &str); 8] = [
    ("linkml", "https://w3id.org/linkml/"),
    ("owl", "http://www.w3.org/2002/07/owl#"),
    ("rdf", "http://www.w3.org/1999/02/22-rdf-syntax-ns#"),
    ("rdfs", "http://www.w3.org/2000/01/rdf-schema#"),
    ("semapv", SEMAPV_NS),
    ("skos", "http://www.w3.org/2004/02/skos/core#"),
    ("sssom", SSSOM_NS),
    ("xsd", "http://www.w3.org/2001/XMLSchema#"),
];

/// The prefix quipu's extension slots are declared under.
pub const QUIPU_PREFIX: &str = "quipu";
/// Property of the `quipu_review` extension slot.
pub const QUIPU_REVIEW_PROPERTY: &str = "alignmentReview";
/// Property of the `quipu_reviewed_by` extension slot.
pub const QUIPU_REVIEWED_BY_PROPERTY: &str = "alignmentReviewedBy";

const DCTERMS: &str = "http://purl.org/dc/terms/";
const PAV: &str = "http://purl.org/pav/";

mod rdf;
pub use rdf::export_turtle;

/// Which SSSOM serialisation to write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    /// SSSOM/TSV with an embedded YAML metadata block.
    Tsv,
    /// SSSOM/RDF as Turtle.
    Turtle,
}

impl ExportFormat {
    /// Parse `tsv`, `ttl`, `turtle` or `rdf`.
    ///
    /// # Errors
    /// Any other value.
    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "tsv" => Ok(Self::Tsv),
            "ttl" | "turtle" | "rdf" => Ok(Self::Turtle),
            other => Err(Error::InvalidValue(format!(
                "unknown SSSOM export format {other:?}; expected tsv or ttl"
            ))),
        }
    }
}

/// What the operator supplies that the working set does not carry.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExportOptions {
    /// The set's `license`, an absolute IRI. Required.
    pub license: String,
    /// Extra CURIE prefixes, merged with the set's own `curie_map`.
    pub prefixes: BTreeMap<String, String>,
    /// A declared prefix under which a bare reviewer name becomes an
    /// `author_id`, e.g. `crew` → `crew:muldoon`.
    pub author_prefix: Option<String>,
}

/// Write `set` in `format`.
///
/// # Errors
/// See [`export_tsv`] and [`export_turtle`].
pub fn export(set: &MappingSet, options: &ExportOptions, format: ExportFormat) -> Result<String> {
    match format {
        ExportFormat::Tsv => export_tsv(set, options),
        ExportFormat::Turtle => export_turtle(set, options),
    }
}

/// An identifier in both of the forms the two serialisations need.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Entity {
    curie: String,
    iri: String,
}

/// The declared prefixes, and compaction against them.
#[derive(Debug)]
struct Prefixes {
    /// Every usable prefix, built-ins included.
    all: BTreeMap<String, String>,
    /// Longest expansion first, then by name, so compaction is deterministic
    /// and picks the most specific namespace.
    by_length: Vec<(String, String)>,
}

impl Prefixes {
    fn new(set: &MappingSet, options: &ExportOptions, extension_slots: bool) -> Result<Self> {
        let mut all: BTreeMap<String, String> = BUILT_IN_PREFIXES
            .iter()
            .map(|(name, iri)| ((*name).to_string(), (*iri).to_string()))
            .collect();
        let mut declare = |name: &str, iri: &str| -> Result<()> {
            if !is_prefix_name(name) {
                return Err(Error::InvalidValue(format!(
                    "CURIE prefix {name:?} is not a valid prefix name"
                )));
            }
            if !is_absolute_iri(iri) {
                return Err(Error::InvalidValue(format!(
                    "CURIE prefix {name:?} expands to {iri:?}, which is not an absolute IRI"
                )));
            }
            match all.get(name) {
                Some(existing) if existing != iri => Err(Error::InvalidValue(format!(
                    "CURIE prefix {name:?} is declared as both {existing:?} and {iri:?}; \
                     refused rather than choosing one"
                ))),
                _ => {
                    all.insert(name.to_string(), iri.to_string());
                    Ok(())
                }
            }
        };
        for (name, iri) in &set.curie_map {
            declare(name, iri)?;
        }
        for (name, iri) in &options.prefixes {
            declare(name, iri)?;
        }
        if extension_slots {
            declare(QUIPU_PREFIX, QUIPU)?;
        }
        let mut by_length: Vec<(String, String)> =
            all.iter().map(|(n, i)| (n.clone(), i.clone())).collect();
        by_length.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then_with(|| a.0.cmp(&b.0)));
        Ok(Self { all, by_length })
    }

    /// The non-built-in prefixes, for the `curie_map`.
    fn declared(&self) -> impl Iterator<Item = (&String, &String)> {
        self.all
            .iter()
            .filter(|(name, _)| !BUILT_IN_PREFIXES.iter().any(|(b, _)| b == name))
    }

    /// Resolve an IRI or a CURIE to both forms.
    fn entity(&self, slot: &str, value: &str) -> Result<Entity> {
        if let Some((prefix, local)) = value.split_once(':')
            && !local.starts_with("//")
            && let Some(expansion) = self.all.get(prefix)
        {
            check_local(slot, value, local)?;
            let iri = format!("{expansion}{local}");
            check_iri(slot, &iri)?;
            return Ok(Entity {
                curie: value.to_string(),
                iri,
            });
        }
        if !is_absolute_iri(value) {
            return Err(Error::InvalidValue(format!(
                "{slot} {value:?} is neither an absolute IRI nor a CURIE with a declared prefix"
            )));
        }
        check_iri(slot, value)?;
        for (name, expansion) in &self.by_length {
            if let Some(local) = value.strip_prefix(expansion.as_str())
                && !local.is_empty()
                && check_local(slot, value, local).is_ok()
            {
                return Ok(Entity {
                    curie: format!("{name}:{local}"),
                    iri: value.to_string(),
                });
            }
        }
        Err(Error::InvalidValue(format!(
            "{slot} {value:?} is not covered by any declared CURIE prefix; SSSOM/TSV \
             requires CURIEs, so declare its namespace (e.g. --prefix name=<namespace>) \
             rather than have one invented"
        )))
    }

    /// Resolve a reviewer: an IRI or CURIE as given, a bare name only under
    /// the operator's `author_prefix`.
    fn agent(&self, slot: &str, value: &str, author_prefix: Option<&str>) -> Result<Entity> {
        if value.contains(':') {
            return self.entity(slot, value);
        }
        let Some(prefix) = author_prefix else {
            return Err(Error::InvalidValue(format!(
                "{slot} {value:?} is a bare name, but SSSOM needs an entity reference; \
                 pass an author prefix (e.g. --author-prefix name with --prefix \
                 name=<namespace>) rather than have quipu invent one"
            )));
        };
        if !self.all.contains_key(prefix) {
            return Err(Error::InvalidValue(format!(
                "author prefix {prefix:?} is not a declared CURIE prefix"
            )));
        }
        self.entity(slot, &format!("{prefix}:{value}"))
    }
}

/// One row with every identifier resolved, checked before anything is
/// written so a refusal never leaves half a file.
struct Row<'a> {
    mapping: &'a Mapping,
    subject: Entity,
    predicate: Entity,
    object: Entity,
    justification: Entity,
    author: Option<Entity>,
    reviewed_by: Option<Entity>,
    confidence: Option<String>,
}

/// The set, validated and resolved.
struct Prepared<'a> {
    set_id: String,
    license: String,
    prefixes: Prefixes,
    rows: Vec<Row<'a>>,
    extension_slots: bool,
}

fn prepare<'a>(set: &'a MappingSet, options: &ExportOptions) -> Result<Prepared<'a>> {
    if !is_absolute_iri(&set.mapping_set_id)
        || check_iri("mapping_set_id", &set.mapping_set_id).is_err()
    {
        return Err(Error::InvalidValue(format!(
            "mapping_set_id {:?} is not an absolute IRI; SSSOM requires one",
            set.mapping_set_id
        )));
    }
    if !is_absolute_iri(&options.license) || check_iri("license", &options.license).is_err() {
        return Err(Error::InvalidValue(format!(
            "license {:?} is not an absolute IRI; SSSOM requires a licence IRI on every \
             mapping set and quipu does not choose one",
            options.license
        )));
    }
    let extension_slots = set
        .mappings
        .iter()
        .any(|m| m.quipu_review.is_some() || m.quipu_reviewed_by.is_some());
    let prefixes = Prefixes::new(set, options, extension_slots)?;

    let mut sorted: Vec<&Mapping> = set.mappings.iter().collect();
    sorted.sort_by(|a, b| {
        a.subject_id
            .cmp(&b.subject_id)
            .then_with(|| a.object_id.cmp(&b.object_id))
            .then_with(|| a.predicate_id.cmp(&b.predicate_id))
    });

    let author_prefix = options.author_prefix.as_deref();
    let mut rows = Vec::with_capacity(sorted.len());
    for mapping in sorted {
        for (slot, label) in [
            ("subject_label", mapping.subject_label.as_deref()),
            ("object_label", mapping.object_label.as_deref()),
        ] {
            if let Some(text) = label
                && text.contains(['\t', '\n', '\r'])
            {
                return Err(Error::InvalidValue(format!(
                    "{slot} {text:?} contains a tab or line break, which SSSOM/TSV cannot carry"
                )));
            }
        }
        if let Some(date) = mapping.mapping_date.as_deref()
            && !is_xsd_date(date)
        {
            return Err(Error::InvalidValue(format!(
                "mapping_date {date:?} is not an xsd:date (YYYY-MM-DD)"
            )));
        }
        let confidence = match mapping.confidence {
            None => None,
            Some(c) if c.is_finite() && (0.0..=1.0).contains(&c) => Some(format!("{c}")),
            Some(c) => {
                return Err(Error::InvalidValue(format!(
                    "confidence {c} for {} / {} is outside SSSOM's 0..1",
                    mapping.subject_id, mapping.object_id
                )));
            }
        };
        rows.push(Row {
            mapping,
            subject: prefixes.entity("subject_id", &mapping.subject_id)?,
            predicate: prefixes.entity("predicate_id", &mapping.predicate_id)?,
            object: prefixes.entity("object_id", &mapping.object_id)?,
            justification: prefixes.entity(
                "mapping_justification",
                mapping.mapping_justification.curie(),
            )?,
            author: mapping
                .author_id
                .as_deref()
                .map(|a| prefixes.agent("author_id", a, author_prefix))
                .transpose()?,
            reviewed_by: mapping
                .quipu_reviewed_by
                .as_deref()
                .map(|a| prefixes.agent("quipu_reviewed_by", a, author_prefix))
                .transpose()?,
            confidence,
        });
    }

    Ok(Prepared {
        set_id: set.mapping_set_id.clone(),
        license: options.license.clone(),
        prefixes,
        rows,
        extension_slots,
    })
}

// ---------------------------------------------------------------- SSSOM/TSV

/// Serialise to standard SSSOM/TSV (embedded metadata, CURIEs throughout).
///
/// Columns with no value in any row are omitted, as the spec recommends; the
/// four required columns are always present.
///
/// # Errors
/// An identifier no declared prefix covers, a bare reviewer name without an
/// author prefix, a missing or relative `license` or `mapping_set_id`, a
/// conflicting prefix declaration, or a value TSV cannot carry.
pub fn export_tsv(set: &MappingSet, options: &ExportOptions) -> Result<String> {
    let p = prepare(set, options)?;
    let mut out = String::new();

    let declared: Vec<_> = p.prefixes.declared().collect();
    if !declared.is_empty() {
        out.push_str("#curie_map:\n");
        for (name, iri) in declared {
            let _ = writeln!(out, "#  {name}: {}", yaml_string(iri));
        }
    }
    if p.extension_slots {
        out.push_str("#extension_definitions:\n");
        for (slot, property, hint) in [
            ("quipu_review", QUIPU_REVIEW_PROPERTY, "xsd:string"),
            (
                "quipu_reviewed_by",
                QUIPU_REVIEWED_BY_PROPERTY,
                "linkml:Uriorcurie",
            ),
        ] {
            let _ = writeln!(out, "#  - slot_name: {slot}");
            let _ = writeln!(out, "#    property: {QUIPU_PREFIX}:{property}");
            let _ = writeln!(out, "#    type_hint: {hint}");
        }
    }
    let _ = writeln!(out, "#license: {}", yaml_string(&p.license));
    let _ = writeln!(out, "#mapping_set_id: {}", yaml_string(&p.set_id));
    out.push_str("#sssom_version: \"1.1\"\n");

    type Cell = for<'r, 's> fn(&'r Row<'s>) -> Option<String>;
    let columns: [(&str, bool, Cell); 12] = [
        ("subject_id", true, |r| Some(r.subject.curie.clone())),
        ("subject_label", false, |r| r.mapping.subject_label.clone()),
        ("predicate_id", true, |r| Some(r.predicate.curie.clone())),
        ("predicate_modifier", false, |r| {
            r.mapping.is_negated().then(|| "Not".to_string())
        }),
        ("object_id", true, |r| Some(r.object.curie.clone())),
        ("object_label", false, |r| r.mapping.object_label.clone()),
        ("mapping_justification", true, |r| {
            Some(r.justification.curie.clone())
        }),
        ("author_id", false, |r| {
            r.author.as_ref().map(|e| e.curie.clone())
        }),
        ("mapping_date", false, |r| r.mapping.mapping_date.clone()),
        ("confidence", false, |r| r.confidence.clone()),
        ("quipu_review", false, |r| {
            r.mapping.quipu_review.map(|v| v.as_str().to_string())
        }),
        ("quipu_reviewed_by", false, |r| {
            r.reviewed_by.as_ref().map(|e| e.curie.clone())
        }),
    ];
    let present: Vec<&(&str, bool, Cell)> = columns
        .iter()
        .filter(|(_, required, cell)| *required || p.rows.iter().any(|r| cell(r).is_some()))
        .collect();

    let header: Vec<&str> = present.iter().map(|(name, _, _)| *name).collect();
    out.push_str(&header.join("\t"));
    out.push('\n');
    for row in &p.rows {
        let cells: Vec<String> = present
            .iter()
            .map(|(_, _, cell)| cell(row).unwrap_or_default())
            .collect();
        out.push_str(&cells.join("\t"));
        out.push('\n');
    }
    Ok(out)
}

/// A YAML double-quoted scalar.
fn yaml_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

// ---------------------------------------------------------------- lexical checks

/// `scheme:rest`, per RFC 3986's scheme production, with a non-empty rest.
fn is_absolute_iri(value: &str) -> bool {
    let Some((scheme, rest)) = value.split_once(':') else {
        return false;
    };
    let mut chars = scheme.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
        && !rest.is_empty()
}

/// Characters an IRI cannot hold unescaped (RFC 3987), which would also break
/// out of a Turtle `<...>`.
fn check_iri(slot: &str, iri: &str) -> Result<()> {
    if iri.chars().any(|c| {
        c.is_whitespace()
            || c.is_control()
            || matches!(c, '<' | '>' | '"' | '{' | '}' | '|' | '\\' | '^' | '`')
    }) {
        return Err(Error::InvalidValue(format!(
            "{slot} {iri:?} contains a character an IRI cannot hold"
        )));
    }
    Ok(())
}

/// A CURIE's local part: non-empty, and nothing TSV or a `|`-separated
/// multi-value list would split on.
fn check_local(slot: &str, value: &str, local: &str) -> Result<()> {
    if local.is_empty()
        || local
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || c == '|')
    {
        return Err(Error::InvalidValue(format!(
            "{slot} {value:?} has no usable CURIE local part"
        )));
    }
    Ok(())
}

/// An XML `NCName`-shaped prefix name: a letter or `_`, then letters, digits,
/// `_`, `-` or `.`.
fn is_prefix_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
}
