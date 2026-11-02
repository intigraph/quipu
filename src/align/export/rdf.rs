//! SSSOM/RDF: the Turtle half of [`super`]. Kept in its own file so each
//! serialisation can be read on its own; it shares the parent's validated
//! validated, prefix-resolved set, so the two formats cannot disagree about
//! what is valid.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use oxrdfio::{RdfFormat, RdfParser};

use crate::error::{Error, Result};

use super::{
    DCTERMS, ExportOptions, PAV, QUIPU_PREFIX, QUIPU_REVIEW_PROPERTY, QUIPU_REVIEWED_BY_PROPERTY,
    prepare,
};
use crate::align::sssom::MappingSet;

/// Serialise to SSSOM/RDF as Turtle.
///
/// The set is a `sssom:MappingSet` named by its `mapping_set_id`; each row is
/// an `owl:Axiom` blank node reached through `sssom:mappings`. Identifiers are
/// written as full IRIs (always valid Turtle, whatever the local part holds)
/// and the CURIE map is declared through `@prefix`, which is how the spec
/// carries it in RDF.
///
/// # Errors
/// Everything [`super::export_tsv`] refuses, and — as a self-check — Turtle that does
/// not parse back.
pub fn export_turtle(set: &MappingSet, options: &ExportOptions) -> Result<String> {
    let p = prepare(set, options)?;
    let mut out = String::new();

    let mut prefixes: BTreeMap<&str, &str> = p
        .prefixes
        .all
        .iter()
        .map(|(n, i)| (n.as_str(), i.as_str()))
        .collect();
    for (name, iri) in [("dcterms", DCTERMS), ("pav", PAV)] {
        match prefixes.get(name) {
            Some(existing) if *existing != iri => {
                return Err(Error::InvalidValue(format!(
                    "CURIE prefix {name:?} is declared as {existing:?}, but SSSOM/RDF \
                     needs it to mean {iri:?}"
                )));
            }
            _ => {
                prefixes.insert(name, iri);
            }
        }
    }
    for (name, iri) in &prefixes {
        let _ = writeln!(out, "@prefix {name}: <{iri}> .");
    }
    out.push('\n');

    let _ = writeln!(out, "<{}> a sssom:MappingSet ;", p.set_id);
    let _ = write!(
        out,
        "    dcterms:license <{}> ;\n    sssom:sssom_version \"1.1\"",
        p.license
    );
    if p.extension_slots {
        for (slot, property, hint) in [
            ("quipu_review", QUIPU_REVIEW_PROPERTY, "xsd:string"),
            (
                "quipu_reviewed_by",
                QUIPU_REVIEWED_BY_PROPERTY,
                "linkml:Uriorcurie",
            ),
        ] {
            let _ = write!(
                out,
                " ;\n    sssom:extension_definitions [ a sssom:ExtensionDefinition ; \
                 sssom:slot_name \"{slot}\" ; sssom:property {QUIPU_PREFIX}:{property} ; \
                 sssom:type_hint {hint} ]"
            );
        }
    }
    if !p.rows.is_empty() {
        let labels: Vec<String> = (1..=p.rows.len()).map(|i| format!("_:m{i}")).collect();
        let _ = write!(out, " ;\n    sssom:mappings {}", labels.join(" , "));
    }
    out.push_str(" .\n");

    for (i, row) in p.rows.iter().enumerate() {
        let m = row.mapping;
        let _ = write!(out, "\n_:m{} a owl:Axiom ;\n", i + 1);
        let _ = writeln!(out, "    owl:annotatedSource <{}> ;", row.subject.iri);
        let _ = writeln!(out, "    owl:annotatedProperty <{}> ;", row.predicate.iri);
        let _ = write!(
            out,
            "    owl:annotatedTarget <{}> ;\n    sssom:mapping_justification <{}>",
            row.object.iri, row.justification.iri
        );
        if m.is_negated() {
            out.push_str(" ;\n    sssom:predicate_modifier sssom:NegatedPredicate");
        }
        if let Some(label) = &m.subject_label {
            let _ = write!(out, " ;\n    sssom:subject_label {}", turtle_string(label));
        }
        if let Some(label) = &m.object_label {
            let _ = write!(out, " ;\n    sssom:object_label {}", turtle_string(label));
        }
        if let Some(author) = &row.author {
            let _ = write!(out, " ;\n    pav:authoredBy <{}>", author.iri);
        }
        if let Some(date) = &m.mapping_date {
            let _ = write!(out, " ;\n    dcterms:created \"{date}\"^^xsd:date");
        }
        if let Some(confidence) = &row.confidence {
            let _ = write!(out, " ;\n    sssom:confidence \"{confidence}\"^^xsd:double");
        }
        if let Some(review) = m.quipu_review {
            let _ = write!(
                out,
                " ;\n    {QUIPU_PREFIX}:{QUIPU_REVIEW_PROPERTY} {}",
                turtle_string(review.as_str())
            );
        }
        if let Some(reviewer) = &row.reviewed_by {
            let _ = write!(
                out,
                " ;\n    {QUIPU_PREFIX}:{QUIPU_REVIEWED_BY_PROPERTY} <{}>",
                reviewer.iri
            );
        }
        out.push_str(" .\n");
    }

    for quad in RdfParser::from_format(RdfFormat::Turtle).for_reader(out.as_bytes()) {
        quad.map_err(|e| Error::InvalidValue(format!("SSSOM/RDF self-check failed: {e}")))?;
    }
    Ok(out)
}

/// A Turtle double-quoted string literal.
fn turtle_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
