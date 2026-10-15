//! Pendant pointer entries in the share manifest (aegis-3fy1kp).
//!
//! Kept beside `share.rs` rather than in it so the share envelope stays inside
//! the source-size bound; the types are re-exported from `crate::share`.

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// A graph a share NAMES without carrying (aegis-3fy1kp, pendant autoload).
///
/// A repository that only points at knowledge ships a share with no facts and one
/// or more pointers. Pointers are declarations, never loads: composition reports
/// them, and following one is the consumer's trust decision (caboodle `packs trust`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SharePointer {
    /// The named graph the pointer is about.
    pub graph: String,
    /// Where that graph is.
    #[serde(flatten)]
    pub target: PointerTarget,
}

/// The two pointer kinds of the pendant autoload design (section 4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum PointerTarget {
    /// A live graph reached through federation, never copied locally.
    Endpoint {
        /// SPARQL endpoint, `http(s)`, with no credentials in the URL.
        endpoint: String,
    },
    /// A pinned pack fetched, verified and composed on first use.
    Pack {
        /// Where the pack archive is fetched from, `http(s)`.
        url: String,
        /// sha256 of the exact archive, `sha256:<64 hex>` or bare hex.
        sha256: String,
    },
}

/// Refuse a malformed or credential-bearing pointer, or two pointers for one graph.
///
/// Run where a manifest is PRODUCED and where one is VERIFIED, so a hand-edited
/// manifest cannot smuggle in what the producer would have refused.
pub(crate) fn validate_pointers(pointers: &[SharePointer]) -> Result<()> {
    let mut seen = std::collections::BTreeSet::new();
    for pointer in pointers {
        oxrdf::NamedNode::new(&pointer.graph).map_err(|e| {
            Error::InvalidValue(format!(
                "share pointer graph {:?} is not an IRI: {e}",
                pointer.graph
            ))
        })?;
        if !seen.insert(pointer.graph.as_str()) {
            return Err(Error::InvalidValue(format!(
                "share pointer graph {} is declared twice",
                pointer.graph
            )));
        }
        let (url, what) = match &pointer.target {
            PointerTarget::Endpoint { endpoint } => (endpoint, "endpoint"),
            PointerTarget::Pack { url, sha256 } => {
                let hex = sha256.strip_prefix("sha256:").unwrap_or(sha256);
                if hex.len() != 64
                    || !hex
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                {
                    return Err(Error::InvalidValue(format!(
                        "share pointer {} has sha256 {sha256:?}; expected 64 lowercase hex digits",
                        pointer.graph
                    )));
                }
                (url, "pack url")
            }
        };
        let rest = url
            .strip_prefix("https://")
            .or_else(|| url.strip_prefix("http://"))
            .filter(|rest| !rest.is_empty())
            .ok_or_else(|| {
                Error::InvalidValue(format!(
                    "share pointer {} {what} {url:?} is not an http(s) URL",
                    pointer.graph
                ))
            })?;
        if rest.contains(['?', '#']) {
            return Err(Error::InvalidValue(format!(
                "share pointer {} {what} carries a query or fragment; a share is published, \
                 and tokens travel there, so it must not",
                pointer.graph
            )));
        }
        if rest.split('/').next().unwrap_or_default().contains('@') {
            return Err(Error::InvalidValue(format!(
                "share pointer {} {what} carries credentials; a share is published, so it \
                 must not",
                pointer.graph
            )));
        }
    }
    Ok(())
}

/// The pointer strings a published manifest carries, one per line, for the
/// outward scrub. `manifest.json` is otherwise exempt (hashes and ids only), so
/// without this an internal hostname in a pointer would leave the LAN unchecked.
pub(crate) fn pointer_scrub_text(pointers: &[SharePointer]) -> String {
    pointers
        .iter()
        .map(|pointer| match &pointer.target {
            PointerTarget::Endpoint { endpoint } => format!("{} {endpoint}\n", pointer.graph),
            PointerTarget::Pack { url, .. } => format!("{} {url}\n", pointer.graph),
        })
        .collect()
}
