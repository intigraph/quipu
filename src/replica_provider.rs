//! Compare portable provider identities in two native composed acquisitions.
//!
//! This is an unwired library component, not a replica readiness endpoint.
//! Both artifacts must come from independently trusted acquisition channels;
//! their hashes check integrity, not origin authentication. MAIN acquisition
//! excludes attachment data, and connection-local generations are not a shared
//! restore epoch or a same-snapshot query guarantee.

use crate::acquisition::composed::{Payload, Provider, Request};
use crate::{Error, Result};
use serde::Serialize;
use std::collections::BTreeMap;

/// A receipt-bound comparison result. Matching providers never grants readiness.
///
/// Fields are private so consumers cannot change `ready` into an authorization.
#[derive(Debug, Serialize)]
pub struct ProviderClosure {
    schema: &'static str,
    provider_closure: &'static str,
    ready: bool,
    source_receipt_sha256: String,
    replica_receipt_sha256: String,
    applied_tx: i64,
    provider_count: usize,
}

impl ProviderClosure {
    /// The remaining runtime consistency and freshness gates are not evaluated.
    #[must_use]
    pub const fn ready(&self) -> bool {
        self.ready
    }
}

fn mismatch() -> Error {
    Error::CannotVerify("portable provider closure or qualified MAIN pin differs".into())
}

fn portable_equal(a: &Provider, b: &Provider) -> bool {
    // `instance` identifies a fenced physical handle on its own host. It must
    // remain bound by each native receipt but is not a cross-host identity.
    a.alias == b.alias
        && a.store_id == b.store_id
        && a.file_sha256 == b.file_sha256
        && a.manifest_sha256 == b.manifest_sha256
        && a.head_tx == b.head_tx
        && a.term_space == b.term_space
        && a.graphs == b.graphs
        && a.exclusion == b.exclusion
}

// Collapsing by alias is safe only because native `verify()` has already
// refused a census with duplicate aliases before this map is built.
fn by_alias(payload: &Payload) -> BTreeMap<&str, &Provider> {
    payload
        .source_receipt
        .composition
        .providers
        .iter()
        .map(|p| (p.alias.as_str(), p))
        .collect()
}

/// Verify two independently expected native receipts, then compare their closure.
///
/// Each request must pin the same exact MAIN transaction and its own previously
/// observed topology generation. The native verifier checks complete payloads,
/// canonical graph bytes, census integrity, selected scope and applied pins.
/// This function adds portable provider equality and policy-dependency equality.
/// It neither opens a store nor acquires, queries, mounts or writes anything.
///
/// A successful result always has `ready: false`. It does not prove attachment
/// query parity, source restore epoch, transport age, search generation, complete
/// read-policy parity, or execution on the future query's exact read snapshot.
///
/// # Errors
///
/// Refuses invalid native payloads, unpinned requests, different portable
/// providers, or different MAIN lineage, scope, content or policy dependencies.
pub fn compare(
    source: &Payload,
    source_request: &Request,
    replica: &Payload,
    replica_request: &Request,
) -> Result<ProviderClosure> {
    source.verify(source_request)?;
    replica.verify(replica_request)?;
    if source_request.tx.is_none()
        || source_request.tx != replica_request.tx
        || source_request.expected_generation.is_none()
        || replica_request.expected_generation.is_none()
    {
        return Err(mismatch());
    }
    let a = &source.source_receipt;
    let b = &replica.source_receipt;
    if a.main.source_store_id != b.main.source_store_id
        || a.main.applied_tx != b.main.applied_tx
        || a.main.graphs != b.main.graphs
        || a.composition.ownership_sha256 != b.composition.ownership_sha256
    {
        return Err(mismatch());
    }
    let left = by_alias(source);
    let right = by_alias(replica);
    if left.len() != right.len()
        || left
            .iter()
            .any(|(alias, a)| right.get(alias).is_none_or(|b| !portable_equal(a, b)))
    {
        return Err(mismatch());
    }
    Ok(ProviderClosure {
        schema: "quipu-provider-closure/comparison-v1",
        provider_closure: "matched",
        ready: false,
        source_receipt_sha256: source.source_receipt_sha256.clone(),
        replica_receipt_sha256: replica.source_receipt_sha256.clone(),
        applied_tx: a.main.applied_tx,
        provider_count: left.len(),
    })
}

#[cfg(all(
    test,
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
#[path = "replica_provider_tests.rs"]
mod tests;
