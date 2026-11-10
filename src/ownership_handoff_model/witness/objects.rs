//! Create-only object backend for the witness (design W2).
//!
//! Every witness record lives under one deterministic key per lineage step,
//! so a conditional create gives exactly one winner per key. The witness logic
//! never relies on a backend-wide lock: checks before a create are only
//! preconditions, and the create-only key is the arbiter.
//!
//! Every doubt is `Unknown`: a timeout, transport error, unexpected status,
//! unreadable body or an exhausted bound. A backend never reports a conflict;
//! the witness resolves `Exists` by reading the exact key back.
use super::Reply;
use super::records::Key;

/// Result of a conditional create.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Created {
    /// This call created the key.
    New,
    /// The key already existed; nothing was written.
    Exists,
}

/// A stored body and the backend's version identifier for it (an object
/// version id or entity tag). The version is evidence, never authority.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Object {
    pub(crate) bytes: Vec<u8>,
    pub(crate) version: String,
}

pub(crate) trait Objects {
    /// Read one key. `Ok(None)` only when the backend positively reports it
    /// absent; any doubt is `Unknown`.
    fn get(&self, key: &str) -> Reply<Option<Object>>;
    /// Create `key` only if absent. A lost reply after a successful create
    /// must surface as `Unknown`, never as `New` or `Exists`.
    fn create(&self, key: &str, bytes: &[u8]) -> Reply<Created>;
    /// Every key of this lineage. More than `limit` keys, a truncated listing
    /// or any doubt is `Unknown`.
    fn list(&self, limit: usize) -> Reply<Vec<String>>;
    /// Deterministic per-operation fault for private fixtures. Real
    /// transports never inject faults; their failures arrive as `Unknown`.
    fn fault(&self) -> Option<super::Fault> {
        None
    }
    /// Read budget for one head walk.
    fn read_budget(&self) -> usize {
        super::records::MAX_READS
    }
}

/// Canonical key name. Fixed width keeps listings unambiguous.
pub(super) fn name(key: Key) -> String {
    match key {
        Key::Active(g) => format!("active/{g:02}"),
        Key::Reserved(g, a) => format!("reserved/{g:02}/{a:02}"),
        Key::Outcome(g, a) => format!("outcome/{g:02}/{a:02}"),
    }
}

/// Strict inverse of [`name`]. Anything else is not a witness key.
pub(super) fn parse(name: &str) -> Option<Key> {
    fn num(s: &str) -> Option<u64> {
        if s.len() != 2 || !s.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        // Width, not the lineage bounds: the walk deliberately probes one
        // key past its last admitted generation/attempt.
        s.parse().ok()
    }
    let parts: Vec<&str> = name.split('/').collect();
    let key = match parts.as_slice() {
        ["active", g] => Key::Active(num(g)?),
        ["reserved", g, a] => Key::Reserved(num(g)?, num(a)?),
        ["outcome", g, a] => Key::Outcome(num(g)?, num(a)?),
        _ => return None,
    };
    (self::name(key) == name).then_some(key)
}
