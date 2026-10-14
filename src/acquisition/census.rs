//! Complete bounded census on the leased active provider handles.
use super::composed::{MAX_PROVIDER_GRAPHS, MAX_PROVIDERS, PROVIDER_BYTES, Provider, digest};
use super::{invalid, provider_fence};
use crate::{Result, Store};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::io::{Read, Seek};
pub(super) fn providers(
    store: &Store,
    fence: &provider_fence::Fence,
    check: &impl Fn() -> Result<()>,
) -> Result<Vec<Provider>> {
    let actual: Vec<(String, String)> = store
        .conn
        .prepare("PRAGMA database_list")?
        .query_map([], |r| Ok((r.get(1)?, r.get(2)?)))?
        .collect::<std::result::Result<_, _>>()?;
    let mut aliases: BTreeSet<_> = actual
        .iter()
        .filter(|(a, _)| a != "main" && a != "temp")
        .map(|(a, _)| a.as_str())
        .collect();
    if aliases.len() != store.attachments.len() || aliases.len() > MAX_PROVIDERS {
        return Err(invalid("incomplete or oversized active provider census"));
    }
    let frozen: Vec<(String, String)> = store
        .conn
        .prepare("SELECT alias,path FROM main.frozen_packs WHERE thawed_at IS NULL")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<std::result::Result<_, _>>()?;
    for (alias, path) in frozen {
        let Some(a) = store.attachments.iter().find(|a| a.alias == alias) else {
            return Err(invalid("frozen provider registry is not mounted"));
        };
        if std::fs::canonicalize(&path).map_err(|e| provider_fence::io(&e))?
            != std::fs::canonicalize(&a.path).map_err(|e| provider_fence::io(&e))?
        {
            return Err(invalid("frozen provider registry changed"));
        }
    }
    let manifests = crate::store::attach::attached_pack_manifests(&store.conn, &store.attachments)
        .map_err(|_| invalid("mounted provider manifests cannot be verified"))?;
    let mut bytes_left = PROVIDER_BYTES;
    let mut spaces = BTreeSet::from([store.local_term_space()?]);
    let mut out = Vec::new();
    for (index, a) in store.attachments.iter().enumerate() {
        check()?;
        crate::store::attach::validate_alias(&a.alias)?;
        if !a.read_only || !aliases.remove(a.alias.as_str()) {
            return Err(invalid("provider census or read-only mode mismatch"));
        }
        provider_fence::mounted_file_unchanged(store, &a.alias)?;
        let Some((_, path)) = actual.iter().find(|(alias, _)| alias == &a.alias) else {
            return Err(invalid("active provider missing"));
        };
        if std::fs::canonicalize(path).map_err(|e| provider_fence::io(&e))?
            != std::fs::canonicalize(&a.path).map_err(|e| provider_fence::io(&e))?
        {
            return Err(invalid("mounted provider path changed"));
        }
        for suffix in ["-wal", "-journal"] {
            match std::fs::metadata(format!("{path}{suffix}")) {
                Ok(m) if m.len() > 0 => {
                    return Err(invalid("provider has mutable journal content"));
                }
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(provider_fence::io(&e)),
            }
        }
        let manifest = manifests
            .iter()
            .find(|(alias, _)| alias == &a.alias)
            .ok_or_else(|| invalid("provider has no immutable pack manifest"))?;
        if !store
            .pack_manifests
            .iter()
            .any(|(alias, m)| alias == &a.alias && m == &manifest.1)
        {
            return Err(invalid("provider manifest changed since mount"));
        }
        // The mounted SQLite handle may retain cached metadata after an
        // external change that preceded this lease. Census a fresh read-only
        // handle to the same fenced inode, not a cached immutable=1 image.
        let fresh = rusqlite::Connection::open_with_flags(
            path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        let fresh_attachment = crate::store::attach::Attachment {
            alias: "main".into(),
            path: path.clone(),
            read_only: true,
        };
        let fresh_manifests =
            crate::store::attach::attached_pack_manifests(&fresh, &[fresh_attachment])
                .map_err(|_| invalid("fenced provider manifest cannot be verified"))?;
        if fresh_manifests.len() != 1 || fresh_manifests[0].1 != manifest.1 {
            return Err(invalid(
                "mounted provider manifest differs from fenced file",
            ));
        }
        let space = crate::store::attach::attached_term_space(&fresh, "main")?;
        if space != crate::store::attach::attached_term_space(&store.conn, &a.alias)? {
            return Err(invalid(
                "mounted provider term space differs from fenced file",
            ));
        }
        if !spaces.insert(space) {
            return Err(invalid("provider term-space collision"));
        }
        let store_id: String = fresh.query_row(
            "SELECT store_id FROM main.store_identity WHERE id=1",
            [],
            |r| r.get(0),
        )?;
        let head_tx: i64 = fresh.query_row(
            "SELECT COALESCE(MAX(id),0) FROM main.transactions",
            [],
            |r| r.get(0),
        )?;
        let mut graphs: Vec<String> = fresh.prepare("SELECT CASE WHEN length(t.iri)<=8192 THEN t.iri ELSE NULL END FROM main.graphs g JOIN main.terms t ON t.id=g.g WHERE g.g<>0 ORDER BY t.iri LIMIT 513")?
            .query_map([],|r|r.get(0))?.collect::<std::result::Result<_,_>>()?;
        if graphs.len() > MAX_PROVIDER_GRAPHS {
            return Err(invalid("provider graph census truncated"));
        }
        graphs.sort();
        graphs.dedup();
        let mut file = fence.file(index)?;
        let size = file.metadata().map_err(|e| provider_fence::io(&e))?.len();
        if size > bytes_left {
            return Err(invalid("provider identity IO cap exceeded"));
        }
        bytes_left -= size;
        file.rewind().map_err(|e| provider_fence::io(&e))?;
        let mut hash = Sha256::new();
        let mut buf = [0_u8; 65536];
        let mut read = 0_u64;
        loop {
            check()?;
            let n = file.read(&mut buf).map_err(|e| provider_fence::io(&e))?;
            if n == 0 {
                break;
            }
            read += n as u64;
            if read > size {
                return Err(invalid("provider size changed"));
            }
            hash.update(&buf[..n]);
        }
        if read != size {
            return Err(invalid("provider identity read incomplete"));
        }
        out.push(Provider {
            alias: a.alias.clone(),
            store_id,
            instance: fence.opaque_id(index)?,
            file_sha256: format!("sha256:{:x}", hash.finalize()),
            manifest_sha256: digest(&manifest.1)?,
            head_tx,
            term_space: space,
            graphs,
            exclusion: "outside-qualified-main-scope".into(),
        });
    }
    fence.check()?;
    Ok(out)
}
