//! Which IRIs the golden-path vocabulary terms are written under.
//!
//! The vocabulary (the predicates Quipu itself mints, such as `stepOf` or
//! `sourceKind`) is separate from the store's `base_ns`, which also names the
//! individuals minted from episode node names. Moving the vocabulary to its
//! published Camayoc namespace must not move those individuals: changing
//! `base_ns` on an existing store would fork the identity of every named entity.
//!
//! [`Vocabulary::Legacy`] (the default) writes every term under `base_ns`,
//! exactly as before. [`Vocabulary::Intigraph`] writes the terms the Camayoc
//! vocabulary has published under their stable `https://w3id.org/intigraph/`
//! IRIs; any other name stays under `base_ns`. Stored data is never rewritten,
//! and old IRIs stay valid through the published `owl:equivalentProperty`
//! bridge.
//!
//! Selecting `intigraph` on a store whose readers still query the legacy IRIs
//! hides new facts from them unless that bridge is live for new writes (reactive
//! materialization, or readers that query both). Switch only after that holds.

use serde::{Deserialize, Serialize};

/// The published Camayoc core vocabulary namespace.
pub const CAMAYOC_CORE_NS: &str = "https://w3id.org/intigraph/camayoc/core/ns#";

/// The vocabulary a store writes its golden-path terms under.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Vocabulary {
    /// Every term under the store's `base_ns` (today's behaviour).
    #[default]
    Legacy,
    /// Published terms under their stable Camayoc IRIs.
    Intigraph,
}

/// Golden-path terms published in the Camayoc core vocabulary under the same
/// local name. Every `PathVocab` term is here; none is a kept-legacy term.
const CAMAYOC_CORE_TERMS: &[&str] = &[
    "about",
    "actionKind",
    "actionTarget",
    "deadEnd",
    "falsifier",
    "omissionAuthority",
    "omissionRuling",
    "omitsStep",
    "omittedStep",
    "outcome",
    "prunedFrom",
    "sourceKind",
    "stepOf",
    "stepOrder",
    "trajectoryOf",
    "verifiedBy",
];

/// The IRI for term `local` in `vocabulary`, with `base_ns` as the fallback.
#[must_use]
pub fn term(local: &str, base_ns: &str, vocabulary: Vocabulary) -> String {
    match vocabulary {
        Vocabulary::Intigraph if CAMAYOC_CORE_TERMS.contains(&local) => {
            format!("{CAMAYOC_CORE_NS}{local}")
        }
        _ => format!("{base_ns}{local}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: &str = "http://example.org/kb/";

    #[test]
    fn legacy_writes_every_term_under_base_ns() {
        for local in CAMAYOC_CORE_TERMS.iter().chain(&["closedAt", "anything"]) {
            assert_eq!(
                term(local, BASE, Vocabulary::Legacy),
                format!("{BASE}{local}")
            );
        }
    }

    #[test]
    fn intigraph_writes_published_terms_under_camayoc_core() {
        assert_eq!(
            term("stepOf", BASE, Vocabulary::Intigraph),
            "https://w3id.org/intigraph/camayoc/core/ns#stepOf"
        );
        for local in CAMAYOC_CORE_TERMS {
            assert_eq!(
                term(local, BASE, Vocabulary::Intigraph),
                format!("{CAMAYOC_CORE_NS}{local}")
            );
        }
    }

    #[test]
    fn intigraph_keeps_unpublished_and_kept_legacy_names_under_base_ns() {
        // closedAt and assignedTo stay legacy permanently; arbitrary episode
        // names are individuals or ad-hoc predicates, never moved.
        for local in ["closedAt", "assignedTo", "checkout", "Service"] {
            assert_eq!(
                term(local, BASE, Vocabulary::Intigraph),
                format!("{BASE}{local}")
            );
        }
    }

    fn path_vocab_iris(v: &crate::path::PathVocab) -> Vec<&str> {
        vec![
            &v.step_of,
            &v.step_order,
            &v.action_kind,
            &v.action_target,
            &v.verified_by,
            &v.falsifier,
            &v.trajectory_of,
            &v.about,
            &v.outcome,
            &v.pruned_from,
            &v.omits_step,
            &v.omitted_step,
            &v.omission_authority,
            &v.omission_ruling,
            &v.dead_end,
            &v.source_kind,
        ]
    }

    #[test]
    fn path_vocab_default_is_byte_identical_legacy() {
        // Deploy-safety gate: a store that never sets `vocabulary` writes
        // exactly the IRIs it wrote before this module existed.
        let legacy = crate::path::PathVocab::new(BASE);
        let iris = path_vocab_iris(&legacy);
        assert_eq!(iris.len(), CAMAYOC_CORE_TERMS.len());
        for iri in &iris {
            let local = iri.strip_prefix(BASE).expect("legacy term under base_ns");
            assert!(
                CAMAYOC_CORE_TERMS.contains(&local),
                "{local} not in the map"
            );
        }
    }

    #[test]
    fn path_vocab_intigraph_moves_every_term_and_nothing_else() {
        let moved = crate::path::PathVocab::with_vocabulary(BASE, Vocabulary::Intigraph);
        for iri in path_vocab_iris(&moved) {
            assert!(
                iri.starts_with(CAMAYOC_CORE_NS),
                "{iri} not under Camayoc core"
            );
        }
    }

    #[test]
    fn config_values_are_lowercase_and_default_is_legacy() {
        assert_eq!(Vocabulary::default(), Vocabulary::Legacy);
        let parsed: Vocabulary = serde_json::from_str("\"intigraph\"").unwrap();
        assert_eq!(parsed, Vocabulary::Intigraph);
        assert!(serde_json::from_str::<Vocabulary>("\"Intigraph\"").is_err());
    }
}
