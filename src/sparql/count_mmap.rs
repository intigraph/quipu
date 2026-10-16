//! Bound scalar COUNT's process working set without changing pooled-reader policy.
//!
//! A count returns no term pointers. Reading a whole index need not map every
//! touched file page into the process (a 256 MB RSS floor despite no bindings).
//! Use the existing bounded SQLite page cache for this scalar statement, then
//! restore the caller's mmap window on success, error, or unwinding. This is
//! connection-local; no persistent store setting or cache is added.
use crate::{Result, Store};
use rusqlite::OptionalExtension;

struct Mapping<'a> {
    store: &'a Store,
    previous: Vec<(String, i64)>,
    previous_cache: Option<i64>,
}
impl Mapping<'_> {
    fn restore(&mut self) -> Result<()> {
        if let Some(size) = self.previous_cache {
            self.store.conn.pragma_update(None, "cache_size", size)?;
            self.previous_cache = None;
        }
        while let Some((schema, size)) = self.previous.last() {
            self.store
                .conn
                .pragma_update(Some(schema), "mmap_size", *size)?;
            self.previous.pop();
        }
        Ok(())
    }
}
impl Drop for Mapping<'_> {
    fn drop(&mut self) {
        if self.restore().is_err() {
            eprintln!("COUNT failed to restore the reader mmap window");
        }
    }
}

pub(crate) fn scalar<T>(store: &Store, evaluate: impl FnOnce() -> Result<T>) -> Result<T> {
    let mut guard = Mapping {
        store,
        previous: Vec::new(),
        previous_cache: None,
    };
    let schemas = store
        .conn
        .prepare("PRAGMA database_list")?
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    for schema in schemas {
        // An in-memory database has no mmap_size row. Capture each provider
        // separately: an unqualified write changes every attached mmap window
        // and would restore them all to the main database's old policy.
        let size: Option<i64> = store
            .conn
            .pragma_query_value(Some(&schema), "mmap_size", |row| row.get(0))
            .optional()?;
        if let Some(size) = size.filter(|size| *size > 0) {
            guard.previous.push((schema.clone(), size));
            store.conn.pragma_update(Some(&schema), "mmap_size", 0)?;
        }
    }
    let cache: i64 = store
        .conn
        .query_row("PRAGMA cache_size", [], |row| row.get(0))?;
    let page_size: i64 = store
        .conn
        .query_row("PRAGMA page_size", [], |row| row.get(0))?;
    let cache_bytes = if cache < 0 {
        cache.saturating_neg().saturating_mul(1024)
    } else {
        cache.saturating_mul(page_size)
    };
    // SQLite also sizes its sorter from the page-cache budget. A scalar COUNT
    // needs neither the reader's large cache nor a graph-sized sort workspace.
    // Preserve smaller caller budgets and restore the connection policy before
    // releasing the lease; leave TEMP storage and alias tables untouched.
    if cache_bytes > 8 * 1024 * 1024 {
        guard.previous_cache = Some(cache);
        store.conn.pragma_update(None, "cache_size", -8192)?;
    }
    let result = evaluate();
    guard.restore()?;
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn composed_provider_mapping_policies_and_alias_table_survive() {
        let dir = tempfile::tempdir().unwrap();
        let main = dir.path().join("main.db");
        let raw = dir.path().join("raw.db");
        let layer = dir.path().join("layer.db");
        drop(Store::open(main.to_str().unwrap()).unwrap());
        let raw_store = Store::open(raw.to_str().unwrap()).unwrap();
        raw_store.graph_create("http://example.org/layer").unwrap();
        drop(raw_store);
        crate::store::respace::respace_file(&raw, &layer, 4).unwrap();
        let store = Store::open_with_attachments(
            main.to_str().unwrap(),
            &[crate::store::attach::Attachment::read_only(
                "layer",
                layer.to_str().unwrap(),
            )],
        )
        .unwrap();
        let size = |schema| {
            store
                .conn
                .pragma_query_value(Some(schema), "mmap_size", |row| row.get::<_, i64>(0))
                .unwrap()
        };
        store
            .conn
            .pragma_update(Some("main"), "mmap_size", 268435456)
            .unwrap();
        store
            .conn
            .pragma_update(Some("layer"), "mmap_size", 67108864)
            .unwrap();
        let aliases = || {
            store
                .conn
                .query_row("SELECT COUNT(*) FROM term_alias", [], |row| {
                    row.get::<_, i64>(0)
                })
                .unwrap()
        };
        let before = aliases();
        for fail in [false, true] {
            let result = scalar(&store, || {
                assert_eq!(size("main"), 0);
                assert_eq!(size("layer"), 0);
                assert_eq!(aliases(), before);
                if fail {
                    Err(crate::Error::InvalidValue("control".into()))
                } else {
                    Ok(())
                }
            });
            assert_eq!(result.is_err(), fail);
            assert_eq!(size("main"), 268435456);
            assert_eq!(size("layer"), 67108864);
            assert_eq!(aliases(), before);
        }
    }

    #[test]
    fn mapped_reader_policy_is_restored_after_success_and_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("store.db");
        drop(Store::open(path.to_str().unwrap()).unwrap());
        let store = Store::open_read_only(path.to_str().unwrap()).unwrap();
        let size = || {
            store
                .conn
                .query_row("PRAGMA mmap_size", [], |r| r.get::<_, i64>(0))
                .unwrap()
        };
        assert_eq!(size(), 268435456);
        let cache = || {
            store
                .conn
                .query_row("PRAGMA cache_size", [], |r| r.get::<_, i64>(0))
                .unwrap()
        };
        store
            .conn
            .pragma_update(None, "cache_size", -32000)
            .unwrap();
        scalar(&store, || {
            assert_eq!(size(), 0);
            assert_eq!(cache(), -8192);
            Ok(())
        })
        .unwrap();
        assert_eq!(size(), 268435456);
        assert_eq!(cache(), -32000);
        assert!(
            scalar::<()>(&store, || {
                assert_eq!(cache(), -8192);
                Err(crate::Error::InvalidValue("control".into()))
            })
            .is_err()
        );
        assert_eq!(size(), 268435456);
        assert_eq!(cache(), -32000);
        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = scalar::<()>(&store, || panic!("unwind control"));
        }));
        assert!(panic.is_err());
        assert_eq!(size(), 268435456);
        assert_eq!(cache(), -32000);
        // Honor a smaller budget and preserve both SQLite representations.
        for requested in [-4096, 512] {
            store
                .conn
                .pragma_update(None, "cache_size", requested)
                .unwrap();
            scalar(&store, || {
                assert_eq!(cache(), requested);
                Ok(())
            })
            .unwrap();
            assert_eq!(cache(), requested);
        }
    }
}
