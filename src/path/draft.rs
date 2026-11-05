//! Draft a `GoldenPath` as Turtle for human review.
//!
//! The same posture as the governance drafting scaffold
//! (`src/governance/draft.rs`): the tool emits a candidate for a human to
//! read, edit, and load — it never writes the store itself. Mechanical
//! omissions come from the cone report (authority `cone-analysis`); human
//! omissions must each name the Decision that made them (authority
//! `human-decision`); dead ends are named explicitly. The result is born a
//! candidate: promotions are separate human acts, and none are drafted here.

use std::fmt::Write as _;

use crate::error::{Error, Result};
use crate::term_vocabulary::{VocabularyMap, term};

use super::cone::{ConeReport, ConeVerdict};

/// What to draft.
#[derive(Debug, Clone)]
pub struct DraftOptions {
    /// Local name for the `GoldenPath` (IRI = `base_ns` + name).
    pub name: String,
    /// Human-readable label.
    pub label: String,
    /// Step IRIs a human chose to omit, each with the Decision IRI that
    /// authorizes the cut.
    pub human_omissions: Vec<(String, String)>,
    /// Step IRIs preserved as dead-end hazards.
    pub dead_ends: Vec<String>,
    /// Namespace the new IRIs (and every unmapped term) live in.
    pub base_ns: String,
    /// The store's vocabulary map: terms and classes are written through it
    /// (see [`crate::term_vocabulary`]). Empty means all under `base_ns`.
    pub vocabulary: VocabularyMap,
}

/// Emit the `GoldenPath` + `PathOmission` Turtle for `cone_report`'s trajectory.
///
/// Mechanically prunable steps (`OutOfCone`) become omissions with authority
/// `cone-analysis`. `CannotEvaluate` steps are NOT omitted — missing data
/// degrades toward keeping the step and telling the human, never toward a
/// silent cut. A human omission of an `InCone` step is accepted (the human
/// overrules the cone — that is what the Decision is for), but omitting a
/// step the cone never saw is refused as a probable typo.
pub fn draft(cone_report: &ConeReport, opts: &DraftOptions) -> Result<String> {
    let known: Vec<&str> = cone_report.steps.iter().map(|s| s.iri.as_str()).collect();
    for (step, _) in &opts.human_omissions {
        if !known.contains(&step.as_str()) {
            return Err(Error::InvalidValue(format!(
                "human omission names a step the cone report does not contain: {step}"
            )));
        }
    }
    for step in &opts.dead_ends {
        if !known.contains(&step.as_str()) {
            return Err(Error::InvalidValue(format!(
                "dead end names a step the cone report does not contain: {step}"
            )));
        }
    }

    let ns = opts.base_ns.as_str();
    let v = super::PathVocab::with_vocabulary(ns, &opts.vocabulary);
    // Compact an IRI under the store's base namespace to the default prefix;
    // a mapped term keeps its full IRI. Quipu declares no prefix of its own.
    let t = |iri: &str| -> String {
        match iri.strip_prefix(ns) {
            Some(local) => format!(":{local}"),
            None => format!("<{iri}>"),
        }
    };
    let class = |local: &str| t(&term(local, ns, &opts.vocabulary));
    let path_iri = format!("{ns}{}", opts.name);
    let mut ttl = String::new();
    let _ = writeln!(ttl, "@prefix : <{ns}> .");
    let _ = writeln!(
        ttl,
        "@prefix rdfs:  <http://www.w3.org/2000/01/rdf-schema#> ."
    );
    let _ = writeln!(ttl);
    let _ = writeln!(
        ttl,
        "# Drafted from {} — review, then load. Nothing was written.",
        cone_report.trajectory
    );
    let _ = writeln!(ttl, "<{path_iri}> a {} ;", class("GoldenPath"));
    let _ = writeln!(ttl, "    rdfs:label {} ;", turtle_str(&opts.label));
    let _ = writeln!(ttl, "    {} \"declared\" ;", t(&v.source_kind));
    let _ = writeln!(
        ttl,
        "    {} <{}> ;",
        t(&v.pruned_from),
        cone_report.trajectory
    );

    let mut omissions: Vec<(String, String, Option<String>)> = Vec::new();
    for step in &cone_report.steps {
        if step.verdict == ConeVerdict::OutOfCone
            && !opts.human_omissions.iter().any(|(s, _)| *s == step.iri)
        {
            omissions.push((step.iri.clone(), "cone-analysis".to_string(), None));
        }
    }
    for (step, decision) in &opts.human_omissions {
        omissions.push((
            step.clone(),
            "human-decision".to_string(),
            Some(decision.clone()),
        ));
    }

    for (i, _) in omissions.iter().enumerate() {
        let _ = writeln!(ttl, "    {} <{path_iri}/omission-{i}> ;", t(&v.omits_step));
    }
    for dead in &opts.dead_ends {
        let _ = writeln!(ttl, "    {} <{dead}> ;", t(&v.dead_end));
    }
    ttl.truncate(ttl.trim_end_matches([' ', '\n', ';']).len());
    ttl.push_str(" .\n");

    for (i, (step, authority, decision)) in omissions.iter().enumerate() {
        let _ = writeln!(ttl);
        let _ = writeln!(
            ttl,
            "<{path_iri}/omission-{i}> a {} ;",
            class("PathOmission")
        );
        let _ = writeln!(
            ttl,
            "    rdfs:label {} ;",
            turtle_str(&format!("omission {i} of {}", opts.label))
        );
        let _ = writeln!(ttl, "    {} \"declared\" ;", t(&v.source_kind));
        let _ = writeln!(ttl, "    {} <{step}> ;", t(&v.omitted_step));
        match decision {
            Some(d) => {
                let _ = writeln!(ttl, "    {} \"{authority}\" ;", t(&v.omission_authority));
                let _ = writeln!(ttl, "    {} <{d}> .", t(&v.omission_ruling));
            }
            None => {
                let _ = writeln!(ttl, "    {} \"{authority}\" .", t(&v.omission_authority));
            }
        }
    }
    Ok(ttl)
}

fn turtle_str(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

#[cfg(test)]
mod tests {
    use super::super::cone::{ConeOptions, cone};
    use super::super::testutil::*;
    use super::*;

    fn report() -> (crate::store::Store, super::super::PathVocab, ConeReport) {
        let (store, vocab) = seed_verified_trajectory();
        let opts = ConeOptions {
            via: vec![PRODUCES.to_string(), CONSUMED_BY.to_string()],
            ..Default::default()
        };
        let r = cone(&store, TRAJ, &vocab, &opts).unwrap();
        (store, vocab, r)
    }

    fn opts() -> DraftOptions {
        DraftOptions {
            name: "gp-deploy".into(),
            label: "golden path: deploy".into(),
            human_omissions: vec![(
                "http://ex/s1-implement".into(),
                "http://ex/decision-prune".into(),
            )],
            dead_ends: vec!["http://ex/s2-detour".into()],
            base_ns: "http://ex/v/".into(),
            vocabulary: VocabularyMap::default(),
        }
    }

    #[test]
    fn drafts_mechanical_and_human_omissions_with_their_authorities() {
        let (_s, _v, r) = report();
        let ttl = draft(&r, &opts()).unwrap();
        assert!(ttl.contains(":prunedFrom <http://ex/traj>"), "{ttl}");
        // s2-detour is OutOfCone -> cone-analysis omission.
        assert!(ttl.contains(":omittedStep <http://ex/s2-detour>"), "{ttl}");
        assert!(ttl.contains("\"cone-analysis\""), "{ttl}");
        // The human cut of the in-cone step carries its Decision.
        assert!(
            ttl.contains(":omittedStep <http://ex/s1-implement>"),
            "{ttl}"
        );
        assert!(
            ttl.contains(":omissionRuling <http://ex/decision-prune>"),
            "{ttl}"
        );
        assert!(ttl.contains(":deadEnd <http://ex/s2-detour>"), "{ttl}");
    }

    #[test]
    fn cannot_evaluate_steps_are_not_silently_cut() {
        let (_s, _v, r) = report();
        let ttl = draft(&r, &opts()).unwrap();
        assert!(
            !ttl.contains(":omittedStep <http://ex/s5-mail>"),
            "a CannotEvaluate step was cut without a human decision:\n{ttl}"
        );
    }

    #[test]
    fn a_typo_in_a_human_omission_is_refused() {
        let (_s, _v, r) = report();
        let mut o = opts();
        o.human_omissions = vec![("http://ex/nope".into(), "http://ex/d".into())];
        assert!(draft(&r, &o).is_err());
    }

    #[test]
    fn the_draft_parses_as_turtle_shaped_output() {
        // Cheap structural check: balanced statements, one GoldenPath block.
        let (_s, _v, r) = report();
        let ttl = draft(&r, &opts()).unwrap();
        assert_eq!(ttl.matches("a :GoldenPath").count(), 1);
        assert_eq!(ttl.matches("a :PathOmission").count(), 2);
    }

    /// Every (predicate, object-IRI) the draft writes, parsed as real Turtle.
    fn parsed(ttl: &str) -> Vec<(String, String)> {
        oxttl::TurtleParser::new()
            .for_reader(ttl.as_bytes())
            .map(|t| {
                let t = t.expect("draft must parse as Turtle");
                (t.predicate.as_str().to_string(), t.object.to_string())
            })
            .collect()
    }

    const RDF_TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";
    const MAP: &str = "@prefix owl: <http://www.w3.org/2002/07/owl#> .\n\
        <http://ex/v/sourceKind> owl:equivalentProperty <https://vocab.example/ns#sourceKind> .\n\
        <http://ex/v/omittedStep> owl:equivalentProperty <https://vocab.example/ns#omittedStep> .\n\
        <http://ex/v/GoldenPath> owl:equivalentClass <https://vocab.example/ns#GoldenPath> .\n";

    #[test]
    fn without_a_map_every_term_and_class_is_under_base_ns() {
        let (_s, _v, r) = report();
        let triples = parsed(&draft(&r, &opts()).unwrap());
        for (predicate, object) in &triples {
            if predicate == RDF_TYPE {
                assert!(object.starts_with("<http://ex/v/"), "class {object}");
            } else if !predicate.starts_with("http://www.w3.org/") {
                assert!(predicate.starts_with("http://ex/v/"), "term {predicate}");
            }
        }
    }

    #[test]
    fn a_draft_writes_terms_and_classes_through_the_store_map() {
        // A drafted path must use the IRIs its own store's readers query.
        let (_s, _v, r) = report();
        let mut o = opts();
        o.vocabulary = VocabularyMap::from_turtle(MAP).unwrap();
        let ttl = draft(&r, &o).unwrap();
        let triples = parsed(&ttl);
        let predicates: Vec<&str> = triples.iter().map(|(p, _)| p.as_str()).collect();
        assert!(
            predicates.contains(&"https://vocab.example/ns#sourceKind"),
            "{ttl}"
        );
        assert!(
            predicates.contains(&"https://vocab.example/ns#omittedStep"),
            "{ttl}"
        );
        assert!(!predicates.contains(&"http://ex/v/sourceKind"), "{ttl}");
        assert!(!predicates.contains(&"http://ex/v/omittedStep"), "{ttl}");
        // An unmapped term stays under base_ns.
        assert!(predicates.contains(&"http://ex/v/prunedFrom"), "{ttl}");
        assert!(
            triples.contains(&(
                RDF_TYPE.to_string(),
                "<https://vocab.example/ns#GoldenPath>".to_string()
            )),
            "{ttl}"
        );
    }

    #[test]
    fn a_draft_declares_only_the_store_namespace() {
        let (_s, _v, r) = report();
        let mut o = opts();
        o.vocabulary = VocabularyMap::from_turtle(MAP).unwrap();
        let ttl = draft(&r, &o).unwrap();
        let prefixes: Vec<&str> = ttl.lines().filter(|l| l.starts_with("@prefix")).collect();
        assert_eq!(
            prefixes,
            vec![
                "@prefix : <http://ex/v/> .",
                "@prefix rdfs:  <http://www.w3.org/2000/01/rdf-schema#> ."
            ]
        );
    }
}
