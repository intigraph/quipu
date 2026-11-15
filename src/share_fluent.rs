//! Manifest-bound opaque fluent evidence. Transport never grants origin authority.
use crate::share::{ShareManifest, manifest_bytes, sha256};
use crate::{Error, Result, Store};
use rusqlite::OptionalExtension;

/// A carrier fits one bounded atomic native admission, including duplicates.
pub const MAX_PROOFS: usize = 64;
/// Match the source witness wire bound without interpreting or authenticating it.
pub const MAX_PROOF_BYTES: usize = 131_072;
const MAX_MANIFEST_BYTES: usize = 32 * 1024 * 1024;

pub(crate) fn validate(proofs: &[String]) -> Result<()> {
    if proofs.len() > MAX_PROOFS
        || proofs
            .iter()
            .any(|p| p.is_empty() || p.len() > MAX_PROOF_BYTES)
    {
        return Err(Error::InvalidValue(
            "fluent proof carrier exceeds bounds".into(),
        ));
    }
    Ok(())
}

/// Build a throwaway inspection view. Exact signed carrier bytes are untouched;
/// syntax inspection is not source authentication or native event admission.
pub(crate) fn inspection_files(
    files: &std::collections::BTreeMap<String, String>,
) -> Result<Option<std::collections::BTreeMap<String, String>>> {
    let Some(manifest) = files.get("manifest.json") else {
        return Ok(None);
    };
    let manifest: serde_json::Value = serde_json::from_str(manifest)
        .map_err(|_| Error::CannotVerify("cannot inspect fluent carrier manifest".into()))?;
    let decode = |name: &str| -> Result<Vec<String>> {
        manifest
            .get(name)
            .map(|value| {
                serde_json::from_value(value.clone()).map_err(|_| {
                    Error::CannotVerify("cannot inspect pending carrier strings".into())
                })
            })
            .transpose()
            .map(Option::unwrap_or_default)
    };
    let mut proofs = decode("fluent_proofs")?;
    validate(&proofs)?;
    let configs = decode("observable_configs")?;
    crate::fluent::pending_config::storage::validate_requests(&configs)?;
    proofs.extend(configs);
    if proofs.is_empty() {
        return Ok(None);
    }
    let mut inspection = files.clone();
    for (index, proof) in proofs.into_iter().enumerate() {
        let value: serde_json::Value = serde_json::from_str(&proof)
            .map_err(|_| Error::CannotVerify("cannot inspect fluent proof JSON".into()))?;
        // Include the complete wire inspection too: JSON object parsing must
        // not hide an escaped identifier in an earlier duplicate field.
        let mut decoded = crate::namespace_publication::decoded(&proof)?;
        decoded.push('\n');
        let mut pending = vec![&value];
        while let Some(value) = pending.pop() {
            match value {
                serde_json::Value::String(text) => {
                    decoded.push_str(text);
                    decoded.push('\n');
                }
                serde_json::Value::Array(values) => pending.extend(values),
                serde_json::Value::Object(fields) => {
                    for (key, value) in fields {
                        decoded.push_str(key);
                        decoded.push('\n');
                        pending.push(value);
                    }
                }
                _ => {}
            }
        }
        for (name, text) in [
            (format!("fluent proof {index} wire"), proof),
            (format!("fluent proof {index} decoded values"), decoded),
        ] {
            inspection
                .entry(name)
                .and_modify(|original| {
                    original.push('\n');
                    original.push_str(&text);
                })
                .or_insert(text);
        }
    }
    Ok(Some(inspection))
}

pub(crate) fn check(manifest: &ShareManifest) -> Result<()> {
    validate(&manifest.fluent_proofs)?;
    crate::fluent::pending_config::storage::validate_requests(&manifest.observable_configs)?;
    if manifest.share_id != sha256(&manifest_bytes(manifest, false)?) {
        return Err(Error::InvalidValue(
            "fluent carrier manifest identity mismatch".into(),
        ));
    }
    Ok(())
}

pub(crate) fn retain(store: &Store, manifest: &ShareManifest) -> Result<()> {
    if manifest.fluent_proofs.is_empty() && manifest.observable_configs.is_empty() {
        return Ok(());
    }
    check(manifest)?;
    let bytes = manifest_bytes(manifest, true)?;
    if bytes.len() > MAX_MANIFEST_BYTES {
        return Err(Error::InvalidValue(
            "fluent carrier manifest exceeds byte bound".into(),
        ));
    }
    let text = String::from_utf8(bytes)
        .map_err(|_| Error::InvalidValue("fluent carrier manifest is not UTF-8".into()))?;
    store.conn.execute_batch("CREATE TABLE IF NOT EXISTS share_fluent_carriers(share_id TEXT PRIMARY KEY, manifest TEXT NOT NULL)")?;
    store.conn.execute("INSERT INTO share_fluent_carriers(share_id,manifest) VALUES(?1,?2) ON CONFLICT(share_id) DO NOTHING", rusqlite::params![manifest.share_id, text])?;
    for raw in &manifest.observable_configs {
        crate::fluent::pending_config::Pending::decode(raw.as_bytes())?.retain(store)?;
    }
    Ok(())
}

/// Read retained, hash-checked evidence. These bytes are not an admission receipt.
pub fn imported_proofs(store: &Store, share_id: &str) -> Result<Option<Vec<String>>> {
    let exists: bool = store.conn.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='share_fluent_carriers')", [], |r| r.get(0))?;
    if !exists {
        return Ok(None);
    }
    let bound = i64::try_from(MAX_MANIFEST_BYTES)
        .map_err(|_| Error::InvalidValue("fluent carrier byte bound overflow".into()))?;
    let text: Option<Option<String>> = store.conn.query_row("SELECT CASE WHEN length(CAST(manifest AS BLOB))<=?2 THEN manifest ELSE NULL END FROM share_fluent_carriers WHERE share_id=?1", rusqlite::params![share_id, bound], |r| r.get(0)).optional()?;
    let text = match text {
        None => return Ok(None),
        Some(Some(text)) => text,
        Some(None) => {
            return Err(Error::InvalidValue(
                "retained fluent carrier exceeds byte bound".into(),
            ));
        }
    };
    let manifest: ShareManifest = serde_json::from_str(&text)
        .map_err(|_| Error::InvalidValue("malformed retained fluent carrier".into()))?;
    if manifest.share_id != share_id {
        return Err(Error::InvalidValue(
            "retained fluent carrier key mismatch".into(),
        ));
    }
    check(&manifest)?;
    Ok(Some(manifest.fluent_proofs))
}

/// Source-only explicit adapter: ordinary transport/promotion never calls this.
/// Native batch rechecks actual importer identity, current governed source scope,
/// exact logical graph/profile and every normal transaction admission gate.
#[cfg(all(not(target_arch = "wasm32"), feature = "shacl"))]
#[allow(dead_code)]
pub(crate) fn admit(store: &mut Store, share_id: &str, graph: i64) -> Result<i64> {
    let proofs = imported_proofs(store, share_id)?
        .ok_or_else(|| Error::InvalidValue("missing pending fluent carrier".into()))?;
    let proofs = proofs
        .iter()
        .map(|p| crate::fluent::source_witness::Proof::decode(p.as_bytes()))
        .collect::<Result<Vec<_>>>()?;
    store.import_fluent_batch_native(&proofs, graph)
}
