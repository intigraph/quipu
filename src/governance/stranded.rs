//! Policies stranded under a namespace that is not the store's own.
//!
//! Governance reads its policies under the store's `base_ns`. A store whose
//! policies were written under a different namespace (an upgrade that set a
//! new `base_ns` over data written under the old compiled-in one) would load
//! none of them, and a guard with no policies denies nothing. Silently. So the
//! registry refuses to build while such policies exist, and names where they
//! are.
//!
//! "Stranded" is deliberately narrow: an entity typed `<other>Policy` that
//! also carries `<other>claim` in the same namespace, i.e. the shape the
//! guard would enforce. A class merely named `Policy` in some unrelated
//! vocabulary does not trip it.

use rusqlite::params;

use crate::error::{Error, Result};
use crate::namespace::RDF_TYPE;
use crate::store::Store;
use crate::types::Value;

/// The first namespace other than `ns` holding a current, enforceable-shaped
/// policy, if any.
pub(crate) fn foreign_policy_namespace(store: &Store, ns: &str) -> Result<Option<String>> {
    let Some(rdf_type) = store.lookup(RDF_TYPE)? else {
        return Ok(None);
    };
    let own = format!("{ns}Policy");
    let classes: Vec<(i64, String)> = store
        .prepare("SELECT id, iri FROM terms WHERE iri LIKE '%Policy'")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<std::result::Result<_, _>>()?;
    for (class, iri) in classes {
        let Some(other) = iri.strip_suffix("Policy") else {
            continue;
        };
        if iri == own || !(other.ends_with('/') || other.ends_with('#')) {
            continue;
        }
        let Some(claim) = store.lookup(&format!("{other}claim"))? else {
            continue;
        };
        let stranded = store
            .prepare(
                "SELECT 1 FROM facts t JOIN facts c ON c.e = t.e \
                 WHERE t.a = ?1 AND t.v = ?2 AND t.op = 1 AND t.valid_to IS NULL \
                 AND c.a = ?3 AND c.op = 1 AND c.valid_to IS NULL LIMIT 1",
            )?
            .exists(params![rdf_type, Value::Ref(class).to_bytes(), claim])?;
        if stranded {
            return Ok(Some(other.to_string()));
        }
    }
    Ok(None)
}

/// Refuse when [`foreign_policy_namespace`] finds stranded policies.
pub(crate) fn refuse_stranded_policies(store: &Store, ns: &str) -> Result<()> {
    match foreign_policy_namespace(store, ns)? {
        None => Ok(()),
        Some(other) => Err(Error::InvalidValue(format!(
            "governance refused: this store holds policies under {other:?}, but its \
             governance namespace is {ns:?}, so they would be silently ignored. Set \
             [quipu] base_ns to {other:?}, or rewrite those policies under {ns:?}."
        ))),
    }
}
