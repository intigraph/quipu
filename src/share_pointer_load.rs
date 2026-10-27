//! First-use pack pointers, shared by native and browser consumers.
//!
//! Declaration never performs I/O. The host supplies its trust decision and
//! bounded archive transport; neither a manifest nor a cached archive grants
//! permission. Only the graph requested by the consumer is followed.

use std::collections::BTreeMap;

use crate::error::{Error, Result};
use crate::share::{PointerTarget, SharePointer};
use crate::share_compose::Composition;
use crate::share_import::ShareImportRequest;
use crate::store::Store;

#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(not(target_arch = "wasm32"))]
pub use native::DirectoryCache;

/// Bounded native archive transport for a first-use resolver callback.
#[cfg(feature = "remote")]
pub fn fetch_archive(url: &str) -> Result<Vec<u8>> {
    crate::share_transport::fetch(url)
}

/// Archive storage keyed by the normalized archive sha256, never by URL.
/// Reads are untrusted: both archive and share identity are checked on every use.
pub trait ArchiveCache {
    /// Read an archive, or report a cache miss. Errors must not become misses.
    fn get(&mut self, digest: &str) -> Result<Option<Vec<u8>>>;
    /// Retain an archive after both integrity checks have succeeded.
    fn put(&mut self, digest: &str, bytes: &[u8]) -> Result<()>;
}

impl ArchiveCache for BTreeMap<String, Vec<u8>> {
    fn get(&mut self, digest: &str) -> Result<Option<Vec<u8>>> {
        Ok(BTreeMap::get(self, digest).cloned())
    }

    fn put(&mut self, digest: &str, bytes: &[u8]) -> Result<()> {
        self.insert(digest.into(), bytes.into());
        Ok(())
    }
}

/// A verified declaration and an explicitly selected, locally trusted authority.
/// Construction verifies manifests but does not fetch or compose any pointer.
pub struct LazyPacks {
    declaration: ShareImportRequest,
    authority: ShareImportRequest,
}

impl LazyPacks {
    /// Register declarations without loading them. The authority is supplied by
    /// the receiver, never selected from the pointer's downloaded contents.
    pub fn new(declaration: ShareImportRequest, authority: ShareImportRequest) -> Result<Self> {
        crate::share_import::verify_share(&declaration)?;
        crate::share_import::verify_share(&authority)?;
        Ok(Self {
            declaration,
            authority,
        })
    }

    /// Look up exactly one declared pack graph; endpoint pointers belong to
    /// federation and are never fetched through this path.
    pub fn pointer(&self, graph: &str) -> Result<&SharePointer> {
        let pointer = self
            .declaration
            .manifest
            .pointers
            .iter()
            .find(|p| p.graph == graph)
            .ok_or_else(|| Error::InvalidValue(format!("pack pointer {graph}: not declared")))?;
        if !matches!(pointer.target, PointerTarget::Pack { .. }) {
            return Err(Error::InvalidValue(format!(
                "pack pointer {graph}: endpoint requires federation"
            )));
        }
        Ok(pointer)
    }

    /// Fetch (or reuse), verify and atomically compose one graph on first use.
    ///
    /// `trust` MUST check whether the pointing repository may load this pointer
    /// under the host's current policy (native hosts use caboodle packs trust).
    /// It runs before cache reads or network access, including on repeat use.
    /// `fetch` must bound its response before buffering (see `share_transport`'s
    /// download limit); this function also rejects oversized supplied buffers.
    /// Nested pointers stay declarations: a later graph use must authorize them.
    /// Neither ROOT nor the receiver's shape registry is changed.
    #[allow(clippy::too_many_arguments)]
    pub fn use_graph(
        &self,
        store: &mut Store,
        graph: &str,
        cache: &mut impl ArchiveCache,
        mut trust: impl FnMut(&ShareImportRequest, &SharePointer) -> Result<()>,
        mut fetch: impl FnMut(&str) -> Result<Vec<u8>>,
        timestamp: &str,
        actor: Option<&str>,
    ) -> Result<Composition> {
        let run = || -> Result<Composition> {
            let pointer = self.pointer(graph)?;
            trust(&self.declaration, pointer)?;
            let PointerTarget::Pack { url, sha256 } = &pointer.target else {
                unreachable!()
            };
            let digest = format!(
                "sha256:{}",
                sha256.strip_prefix("sha256:").unwrap_or(sha256)
            );
            let cached = cache.get(&digest)?;
            let miss = cached.is_none();
            let bytes = match cached {
                Some(bytes) => bytes,
                None => fetch(url)?,
            };
            if bytes.len() > crate::share_transport::MAX_SHARE_DOWNLOAD_BYTES {
                return Err(Error::InvalidValue(
                    "archive exceeds download byte limit".into(),
                ));
            }
            if crate::share::sha256(&bytes) != digest {
                return Err(Error::InvalidValue("archive sha256 mismatch".into()));
            }
            let gzip = bytes.starts_with(&[0x1f, 0x8b]);
            let mut request = crate::share_transport::read_archive_bytes(&bytes, url, gzip)?;
            crate::share_import::verify_share(&request)?;
            // Destination is receiver policy. Never inherit an exemption from
            // a downloaded manifest; preserve the declaring receiver's choice.
            request.destination = self.declaration.destination.clone();
            if miss {
                cache.put(&digest, &bytes)?;
            }
            let requests = [self.authority.clone(), self.declaration.clone(), request];
            store.ensure_ownership_writable()?;
            store.conn.execute_batch("SAVEPOINT quipu_pointer_use")?;
            let result = crate::share_compose::compose(store, &requests, Some(0), timestamp, actor)
                .and_then(|result| {
                    if result.outcome != "composed" {
                        return Err(Error::InvalidValue(format!(
                            "composition quarantined: {}",
                            result.validation
                        )));
                    }
                    Ok(result)
                });
            match result {
                Ok(result) => {
                    store.conn.execute_batch("RELEASE quipu_pointer_use")?;
                    Ok(result)
                }
                Err(error) => {
                    store.conn.execute_batch(
                        "ROLLBACK TO quipu_pointer_use; RELEASE quipu_pointer_use",
                    )?;
                    store.read_model.borrow_mut().clear();
                    store.term_cache.borrow_mut().clear_persistent();
                    Err(error)
                }
            }
        };
        let mut run = run;
        run().map_err(|e| Error::InvalidValue(format!("pack pointer {graph}: {e}")))
    }
}

#[cfg(all(test, feature = "shacl", not(target_arch = "wasm32")))]
mod tests;
