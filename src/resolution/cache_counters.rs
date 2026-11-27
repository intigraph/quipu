//! Process-wide label-index counters (aegis-wjo7).
//!
//! `/resolve` either reuses the shared label snapshot, catches it up from the
//! transaction delta, or does a full label scan (~360k labels in production).
//! Which one it did was invisible, so a slow `/resolve` could not be told apart
//! from a cache that kept being emptied. These counters record every outcome
//! and every reason the commit hook emptied the shared cache. Counting is a
//! relaxed atomic add; nothing here changes which path is taken.
use std::sync::atomic::{AtomicU64, Ordering};

/// Where one label-index request was answered from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lookup {
    /// The resident snapshot was current.
    Hit,
    /// A same-generation snapshot was caught up from the delta.
    CatchUp,
    /// No resident snapshot: full scan.
    ScanCold,
    /// A resident snapshot the delta could not certify: full scan.
    ScanUncertified,
    /// Caching is off, or the store is composed (attachments): full scan.
    ScanDisabled,
    /// Inside an open transaction: full scan.
    ScanInTransaction,
    /// The store generation could not be read: full scan.
    ScanKeyError,
}

/// What one off-request warm did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Warm {
    /// The resident snapshot was current or caught up.
    Current,
    /// A full scan was published.
    Published,
    /// A full scan was discarded because a write landed during it.
    Discarded,
}

/// Why the commit hook left the shared cache empty after a commit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cleared {
    /// The commit's effective datums were unknown.
    UnknownEffects,
    /// Caching off or a transaction still open.
    NotCacheable,
    /// The resident snapshot was not this commit's direct predecessor
    /// (interleaved commits, another store, epoch or attachment change).
    NotPredecessor,
    /// The commit changed an `owl:sameAs` identity.
    SameAs,
    /// Patching failed with an error.
    Error,
}

const LOOKUPS: [(Lookup, &str); 7] = [
    (Lookup::Hit, "hit"),
    (Lookup::CatchUp, "catch_up"),
    (Lookup::ScanCold, "scan_cold"),
    (Lookup::ScanUncertified, "scan_uncertified"),
    (Lookup::ScanDisabled, "scan_disabled"),
    (Lookup::ScanInTransaction, "scan_in_transaction"),
    (Lookup::ScanKeyError, "scan_key_error"),
];
const WARMS: [(Warm, &str); 3] = [
    (Warm::Current, "current"),
    (Warm::Published, "published"),
    (Warm::Discarded, "discarded"),
];
const CLEARS: [(Cleared, &str); 5] = [
    (Cleared::UnknownEffects, "unknown_effects"),
    (Cleared::NotCacheable, "not_cacheable"),
    (Cleared::NotPredecessor, "not_predecessor"),
    (Cleared::SameAs, "same_as"),
    (Cleared::Error, "error"),
];

static LOOKUP_COUNTS: [AtomicU64; 7] = [const { AtomicU64::new(0) }; 7];
static WARM_COUNTS: [AtomicU64; 3] = [const { AtomicU64::new(0) }; 3];
static CLEAR_COUNTS: [AtomicU64; 5] = [const { AtomicU64::new(0) }; 5];

fn bump<T: PartialEq>(table: &[(T, &str)], counts: &[AtomicU64], which: &T) {
    if let Some(i) = table.iter().position(|(t, _)| t == which) {
        counts[i].fetch_add(1, Ordering::Relaxed);
    }
}

pub(crate) fn lookup(which: Lookup) {
    bump(&LOOKUPS, &LOOKUP_COUNTS, &which);
}

pub(crate) fn warm(which: Warm) {
    bump(&WARMS, &WARM_COUNTS, &which);
}

pub(crate) fn cleared(which: Cleared) {
    bump(&CLEARS, &CLEAR_COUNTS, &which);
}

fn read<T>(table: &[(T, &'static str)], counts: &[AtomicU64]) -> Vec<(&'static str, u64)> {
    table
        .iter()
        .zip(counts)
        .map(|((_, label), n)| (*label, n.load(Ordering::Relaxed)))
        .collect()
}

/// `(outcome, count)` for every label-index lookup outcome, in a fixed order.
pub fn lookup_counts() -> Vec<(&'static str, u64)> {
    read(&LOOKUPS, &LOOKUP_COUNTS)
}

/// `(outcome, count)` for every off-request warm outcome.
pub fn warm_counts() -> Vec<(&'static str, u64)> {
    read(&WARMS, &WARM_COUNTS)
}

/// `(reason, count)` for every reason the commit hook emptied the cache.
pub fn cleared_counts() -> Vec<(&'static str, u64)> {
    read(&CLEARS, &CLEAR_COUNTS)
}

/// One count of `label` in `counts`, for tests.
#[cfg(test)]
pub(crate) fn count(counts: &[(&'static str, u64)], label: &str) -> u64 {
    counts
        .iter()
        .find(|(l, _)| *l == label)
        .map_or(0, |(_, n)| *n)
}
