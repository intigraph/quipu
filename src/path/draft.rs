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
use crate::term_vocabulary::{CAMAYOC_CORE_NS, Vocabulary};

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
    /// Namespace the new IRIs (and, under the legacy vocabulary, the terms)
    /// live in.
    pub base_ns: String,
    /// Which IRIs the golden-path terms are written under; see
    /// [`crate::term_vocabulary`]. Classes and new IRIs stay under `base_ns`.
    pub vocabulary: crate::term_vocabulary::Vocabulary,
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
    let v = super::PathVocab::with_vocabulary(ns, opts.vocabulary);
    // Compact a term IRI with the prefix it lives under. Legacy terms keep the
    // `aegis:` form, so a legacy draft is byte-identical to before.
    let t = |iri: &str| -> String {
        if let Some(local) = iri.strip_prefix(ns) {
            format!("aegis:{local}")
        } else if let Some(local) = iri.strip_prefix(CAMAYOC_CORE_NS) {
            format!("core:{local}")
        } else {
            format!("<{iri}>")
        }
    };
    let path_iri = format!("{ns}{}", opts.name);
    let mut ttl = String::new();
    let _ = writeln!(ttl, "@prefix aegis: <{ns}> .");
    if opts.vocabulary != Vocabulary::Legacy {
        let _ = writeln!(ttl, "@prefix core:  <{CAMAYOC_CORE_NS}> .");
    }
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
    let _ = writeln!(ttl, "<{path_iri}> a aegis:GoldenPath ;");
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
        let _ = writeln!(ttl, "<{path_iri}/omission-{i}> a aegis:PathOmission ;");
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
            vocabulary: Vocabulary::Legacy,
        }
    }

    #[test]
    fn drafts_mechanical_and_human_omissions_with_their_authorities() {
        let (_s, _v, r) = report();
        let ttl = draft(&r, &opts()).unwrap();
        assert!(ttl.contains("aegis:prunedFrom <http://ex/traj>"), "{ttl}");
        // s2-detour is OutOfCone -> cone-analysis omission.
        assert!(
            ttl.contains("aegis:omittedStep <http://ex/s2-detour>"),
            "{ttl}"
        );
        assert!(ttl.contains("\"cone-analysis\""), "{ttl}");
        // The human cut of the in-cone step carries its Decision.
        assert!(
            ttl.contains("aegis:omittedStep <http://ex/s1-implement>"),
            "{ttl}"
        );
        assert!(
            ttl.contains("aegis:omissionRuling <http://ex/decision-prune>"),
            "{ttl}"
        );
        assert!(ttl.contains("aegis:deadEnd <http://ex/s2-detour>"), "{ttl}");
    }

    #[test]
    fn cannot_evaluate_steps_are_not_silently_cut() {
        let (_s, _v, r) = report();
        let ttl = draft(&r, &opts()).unwrap();
        assert!(
            !ttl.contains("aegis:omittedStep <http://ex/s5-mail>"),
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
        assert_eq!(ttl.matches("a aegis:GoldenPath").count(), 1);
        assert_eq!(ttl.matches("a aegis:PathOmission").count(), 2);
    }

    /// Every predicate the draft writes, parsed as real Turtle.
    fn predicates(ttl: &str) -> std::collections::BTreeSet<String> {
        oxttl::TurtleParser::new()
            .for_reader(ttl.as_bytes())
            .map(|t| {
                t.expect("draft must parse as Turtle")
                    .predicate
                    .as_str()
                    .to_string()
            })
            .collect()
    }

    /// The term predicates a draft may use, resolved for `vocabulary`.
    fn term_iris(vocabulary: Vocabulary) -> Vec<String> {
        let v = super::super::PathVocab::with_vocabulary("http://ex/v/", vocabulary);
        vec![
            v.source_kind,
            v.pruned_from,
            v.omits_step,
            v.dead_end,
            v.omitted_step,
            v.omission_authority,
            v.omission_ruling,
        ]
    }

    #[test]
    fn a_draft_writes_exactly_the_vocabulary_readers_query() {
        // A drafted path must be readable by the same PathVocab the store uses,
        // under either vocabulary, or a loaded draft is invisible to its readers.
        let (_s, _v, r) = report();
        for vocabulary in [Vocabulary::Legacy, Vocabulary::Intigraph] {
            let mut o = opts();
            o.vocabulary = vocabulary;
            let used = predicates(&draft(&r, &o).unwrap());
            for iri in term_iris(vocabulary) {
                assert!(used.contains(&iri), "{vocabulary:?}: draft lacks {iri}");
            }
            let other = match vocabulary {
                Vocabulary::Legacy => Vocabulary::Intigraph,
                Vocabulary::Intigraph => Vocabulary::Legacy,
            };
            for iri in term_iris(other) {
                assert!(!used.contains(&iri), "{vocabulary:?}: draft wrote {iri}");
            }
        }
    }

    #[test]
    fn a_legacy_draft_declares_no_extra_prefix() {
        let (_s, _v, r) = report();
        let ttl = draft(&r, &opts()).unwrap();
        assert!(!ttl.contains("@prefix core:"), "{ttl}");
    }
}
