//! The outward boundary: which destination a share is bound for, and the
//! block-tier identifier scrub that a share bound outward must pass.
//!
//! Split from `share.rs` because three producers need it — a full share, a
//! delta, and an import of a share someone else marked — and because the
//! destination and the check it governs are one decision, not two.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::store::Store;

/// Where a share is bound, and therefore whether the outward scrub applies.
///
/// The default is [`ShareDestination::Outward`] and it is load-bearing: every
/// caller that does not name a destination — including every existing one, and
/// every HTTP request — gets the scrub. `Internal` has to be asked for by name.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ShareDestination {
    /// Anywhere, including a public remote. The outward scrub applies.
    #[default]
    Outward,
    /// A LAN-internal destination only. The outward scrub is skipped, and the
    /// manifest is stamped so the exemption travels with the payload.
    Internal,
    /// Audience-bound owner-authorized transfer to an exact receiver.
    #[serde(untagged)]
    Audience(crate::entity_namespaces::DestinationBinding),
}

impl ShareDestination {
    /// Whether this destination skips the outward scrub.
    #[must_use]
    pub fn is_internal(&self) -> bool {
        matches!(self, Self::Internal | Self::Audience(_))
    }
}

/// The flag an operator passes to accept an internal destination, named in
/// every refusal so a reader is never left to guess the escape hatch exists.
pub const INTERNAL_FLAG: &str = "--destination internal";

fn outward_scrub_patterns(store: &Store) -> Result<Vec<(String, regex::Regex)>> {
    const PREFIXES: &str = "PREFIX aegis: <http://aegis.gastown.local/ontology/> \
        PREFIX rdfs: <http://www.w3.org/2000/01/rdf-schema#>";
    const PATTERN: &str = "?rule a aegis:InternalIdentifierPattern ; \
        rdfs:label ?label ; aegis:regex ?regex ; aegis:enforcementTier \"block\" .";
    // Read complete rules in ROOT and in each named graph. Do not merge
    // partial rule statements from unrelated graphs into a new policy.
    let query = format!(
        "{PREFIXES} SELECT DISTINCT ?label ?regex WHERE {{ \
        {{ {PATTERN} }} UNION {{ GRAPH ?catalogue {{ {PATTERN} }} }} \
        }} ORDER BY ?label ?regex"
    );
    let crate::sparql::QueryResult::Select { rows, .. } = crate::sparql::query(store, &query)?
    else {
        return Err(Error::Store(
            "share scrub: InternalIdentifierPattern query did not return rows".into(),
        ));
    };
    if rows.is_empty() {
        return Err(Error::CannotVerify(format!(
            "share scrub found no block-tier patterns in ROOT or named graphs; \
             load an identifier-policy catalogue before sharing outward. \
             Use {INTERNAL_FLAG} only for an internal destination."
        )));
    }
    rows.into_iter()
        .map(|row| {
            let label = match row.get("label") {
                Some(crate::types::Value::Str(value)) => value.clone(),
                _ => {
                    return Err(Error::Store(
                        "share scrub: pattern has no string label".into(),
                    ));
                }
            };
            let Some(crate::types::Value::Str(source)) = row.get("regex") else {
                return Err(Error::Store(format!(
                    "share scrub: pattern {label:?} has no string regex"
                )));
            };
            let compiled = regex::Regex::new(source).map_err(|error| {
                Error::InvalidValue(format!(
                    "share scrub: pattern {label:?} has invalid regex: {error}"
                ))
            })?;
            Ok((label, compiled))
        })
        .collect()
}

/// Refuse a payload carrying a block-tier internal identifier.
///
/// `context` names the boundary being crossed so a refusal says which command
/// refused; the flag is named unconditionally, because a guard that reports a
/// dead end sends the operator looking for a silent way round it.
///
/// # Errors
/// The pattern catalogue cannot be read, or a payload file matches a block-tier
/// pattern.
pub(crate) fn scrub_outward_payload(
    store: &Store,
    files: &BTreeMap<String, String>,
    context: &str,
) -> Result<()> {
    for (label, pattern) in outward_scrub_patterns(store)? {
        for (name, contents) in files {
            if name == "manifest.json" {
                continue;
            }
            if let Some(hit) = pattern.find(contents) {
                return Err(Error::PolicyDenied(format!(
                    "{context} refused {name}: InternalIdentifierPattern {label:?} matched bytes {}..{}; \
                     identifiers are entity identity and are never rewritten at this boundary. \
                     Pass {INTERNAL_FLAG} if this share is bound for a LAN-internal destination only.",
                    hit.start(),
                    hit.end()
                )));
            }
        }
    }
    Ok(())
}

/// Apply the outward scrub unless the destination is explicitly internal.
///
/// The ONE place the exemption is expressed. Producers call this rather than
/// branching themselves, so a new producer cannot acquire a silent opt-out by
/// forgetting to check — it has to pass a destination in.
///
/// # Errors
/// The payload is bound outward and fails [`scrub_outward_payload`].
pub(crate) fn enforce_destination(
    store: &Store,
    files: &BTreeMap<String, String>,
    destination: ShareDestination,
    context: &str,
) -> Result<()> {
    // Internal transport keeps opaque bytes pending; outward/audience transfer
    // inspects decoded nested strings without rewriting signed evidence.
    let inspection = if matches!(destination, ShareDestination::Internal) {
        None
    } else {
        crate::share_fluent::inspection_files(files)?
    };
    let files = inspection.as_ref().unwrap_or(files);
    crate::namespace_publication::enforce(store, files, &destination)?;
    match destination {
        ShareDestination::Audience(binding) => crate::namespace_publication::scrub_audience(
            store,
            files,
            &ShareDestination::Audience(binding),
        ),
        ShareDestination::Internal => Ok(()),
        ShareDestination::Outward => scrub_outward_payload(store, files, context),
    }
}

/// Declared provenance keys that are operational, never carried outward.
///
/// A field rule, not a pattern: it does not depend on the identifier catalogue,
/// which may be empty on a store (write-provenance-persistence §6).
pub const OPERATIONAL_DECLARATIONS: [&str; 2] = ["host", "session"];

/// Whether carried declarations must be scrubbed for `destination`.
///
/// Only an explicitly `Internal` destination keeps `host` and `session`; an
/// `Audience` transfer leaves this store like an outward one does, so it is
/// scrubbed too, unlike the regex exemption of [`ShareDestination::is_internal`].
#[must_use]
pub fn scrubs_declarations(destination: &ShareDestination) -> bool {
    !matches!(destination, ShareDestination::Internal)
}

/// The keys a manifest records as `declarations_scrubbed` for `destination`,
/// so a receiver can tell a scrubbed declaration from an undeclared one.
#[must_use]
pub fn declarations_scrubbed(destination: &ShareDestination) -> Vec<String> {
    if scrubs_declarations(destination) {
        OPERATIONAL_DECLARATIONS
            .iter()
            .map(|k| (*k).to_string())
            .collect()
    } else {
        Vec::new()
    }
}

/// Remove `host` and `session` from one carried declaration, structurally and
/// before serialization, unless `destination` is explicitly internal.
///
/// The ONE place the field rule is expressed, beside [`enforce_destination`],
/// so a producer that carries declarations passes its destination here rather
/// than branching itself. The regex scrub still applies to whatever remains.
pub fn scrub_declarations(destination: &ShareDestination, declared: &mut BTreeMap<String, String>) {
    if scrubs_declarations(destination) {
        for key in OPERATIONAL_DECLARATIONS {
            declared.remove(key);
        }
    }
}

/// The receiver half of the rule: a non-internal artifact carrying `host` or
/// `session` in a declaration was not produced under it, and is refused rather
/// than imported, the way a self-registered attestation is.
///
/// `declared` is one record's carried declaration object, if any.
///
/// # Errors
/// [`Error::PolicyDenied`] naming the offending key.
pub fn refuse_unscrubbed(internal: bool, declared: Option<&serde_json::Value>) -> Result<()> {
    if internal {
        return Ok(());
    }
    let Some(declared) = declared else {
        return Ok(());
    };
    let Some(map) = declared.as_object() else {
        return Err(Error::PolicyDenied(
            "carried declarations must be a JSON object".into(),
        ));
    };
    for key in OPERATIONAL_DECLARATIONS {
        if map.contains_key(key) {
            return Err(Error::PolicyDenied(format!(
                "a non-internal artifact carries the operational declaration {key:?}; \
                 its producer did not apply the outward scrub, so it is refused on import"
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
pub(crate) fn seed_test_catalogue(store: &mut Store) {
    let graph = store.overlay_create("urn:test:catalogue", 0).unwrap();
    crate::rdf::ingest_rdf_to_graph(
        store,
        include_bytes!("../tests/fixtures/share-catalogue.ttl").as_slice(),
        oxrdfio::RdfFormat::Turtle,
        None,
        "2026-08-01T00:00:00Z",
        None,
        Some("test-catalogue"),
        graph,
    )
    .unwrap();
}

#[cfg(test)]
#[path = "share_scrub_tests.rs"]
mod tests;
