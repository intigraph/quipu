use super::*;
use crate::ownership_handoff_model::{Authority, AuthorityUnit, Journal, Placement, Request};
use crate::store::Datum;
use crate::types::{Op, Value};
use std::path::PathBuf;

const TS: &str = "2026-11-07T00:00:00Z";
const SHAPES: &str = "@prefix sh: <http://www.w3.org/ns/shacl#> .\n<urn:scope:Shape> a sh:NodeShape ; sh:targetClass <urn:scope:C> .";

struct Fixture {
    _dir: tempfile::TempDir,
    store: Store,
    path: PathBuf,
    journal: PathBuf,
    authority: Authority,
}

impl Fixture {
    fn new() -> Self {
        Self::configured(|_| {})
    }

    fn configured(configure: impl FnOnce(&mut Store)) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("store.db");
        let journal = dir.path().join("lineage.journal");
        let mut store = Store::open(path.to_str().unwrap()).unwrap();
        store
            .conn
            .execute_batch("CREATE TABLE scope_probe(value TEXT)")
            .unwrap();
        configure(&mut store);
        let authority = Authority {
            store_id: store.store_id().unwrap(),
            unit: AuthorityUnit::FullStore,
            generation: 0,
            epoch: "epoch-A".into(),
            owner: Placement {
                principal: "fixture-A".into(),
                native: [1; 32],
            },
            policy: "existing-policy".into(),
            predecessor: None,
        };
        super::super::ownership_journal::create(
            &journal,
            &Journal::Active {
                authority: authority.clone(),
            },
        )
        .unwrap();
        drop(store);
        let store = super::super::ownership_admission_fixture::open(&path, &journal, &authority);
        Self {
            _dir: dir,
            store,
            path,
            journal,
            authority,
        }
    }

    fn active(&self) -> Journal {
        Journal::Active {
            authority: self.authority.clone(),
        }
    }

    fn retiring(&self) -> Journal {
        Journal::Retiring {
            request: Request {
                operation: "fixture-op".into(),
                operator: "existing-operator".into(),
                source: self.authority.clone(),
                successor_generation: 1,
                destination: Placement {
                    principal: "fixture-B".into(),
                    native: [2; 32],
                },
                profile: "existing-profile".into(),
            },
        }
    }
}

fn write(store: &mut Store, tag: &str) -> Result<i64> {
    let datum = Datum {
        entity: store.intern(&format!("urn:scope:{tag}"))?,
        attribute: store.intern("urn:scope:p")?,
        value: Value::Int(1),
        valid_from: TS.into(),
        valid_to: None,
        op: Op::Assert,
    };
    store.transact(&[datum], TS, None, None)
}

/// Compare every table's actual values, not just row counts. This covers the
/// real fact, cursor, audit and receipt tables without copying their writers.
fn snapshot(store: &Store) -> Vec<(String, String, Vec<String>)> {
    let tables: Vec<(String, String)> = store
        .conn
        .prepare(
            "SELECT name, coalesce(sql,'') FROM sqlite_master WHERE type='table' ORDER BY name",
        )
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    tables
        .into_iter()
        .map(|(name, schema)| {
            let sql = format!("SELECT * FROM \"{}\"", name.replace('"', "\"\""));
            let mut statement = store.conn.prepare(&sql).unwrap();
            let columns = statement.column_count();
            let mut rows: Vec<String> = statement
                .query_map([], |r| {
                    (0..columns)
                        .map(|i| r.get::<_, rusqlite::types::Value>(i))
                        .collect::<rusqlite::Result<Vec<_>>>()
                        .map(|values| format!("{values:?}"))
                })
                .unwrap()
                .collect::<rusqlite::Result<_>>()
                .unwrap();
            rows.sort();
            (name, schema, rows)
        })
        .collect()
}

#[test]
fn real_autocommit_commit_and_release_hold_the_same_permit() {
    for mode in ["autocommit", "commit", "release"] {
        let mut f = Fixture::new();
        f.store
            .ownership_operation(|s| {
                let sql = match mode {
                    "autocommit" => "INSERT INTO scope_probe VALUES ('positive')",
                    "commit" => "BEGIN; INSERT INTO scope_probe VALUES ('positive'); COMMIT",
                    _ => "SAVEPOINT p; INSERT INTO scope_probe VALUES ('positive'); RELEASE p",
                };
                s.conn.execute_batch(sql)?;
                Ok(())
            })
            .unwrap();
        assert!(f.store.conn.is_autocommit(), "{mode}");
        assert!(
            ownership_latch::exclusive(&f.journal).is_ok(),
            "permit released after {mode}"
        );
        let before = snapshot(&f.store);
        assert!(
            f.store
                .conn
                .execute("INSERT INTO scope_probe VALUES ('unscoped')", [])
                .is_err(),
            "unscoped pinned write committed without a permit"
        );
        assert_eq!(snapshot(&f.store), before, "unscoped pinned write");
        super::super::ownership_journal::advance(&f.journal, &f.active(), &f.retiring()).unwrap();
        assert!(
            f.store
                .ownership_operation(|s| {
                    s.conn
                        .execute("INSERT INTO scope_probe VALUES ('stale')", [])?;
                    Ok(())
                })
                .is_err()
        );
        assert_eq!(snapshot(&f.store), before, "{mode}: retirement refusal");
    }
}

#[test]
fn atomic_scope_rollback_discards_rows_cursors_audit_receipts_and_term_cache() {
    let mut f = Fixture::new();
    let before = snapshot(&f.store);
    let result: Result<i64> = f.store.ownership_transaction(|s| {
        let receipt = write(s, "rollback")?;
        s.commit_consumer("scope-consumer", receipt, TS)?;
        s.conn
            .execute("INSERT INTO scope_probe VALUES ('tentative')", [])?;
        Err(denied("fixture failure before outer RELEASE"))
    });
    assert!(result.is_err(), "no success receipt escapes");
    assert!(f.store.conn.is_autocommit());
    assert_eq!(snapshot(&f.store), before);
    assert_eq!(f.store.lookup("urn:scope:rollback").unwrap(), None);
    f.store
        .ownership_transaction(|s| write(s, "positive"))
        .unwrap();
    assert_ne!(snapshot(&f.store), before);
}

#[test]
fn atomic_body_cannot_commit_early_and_unwind_cleans_the_actual_transaction() {
    let mut f = Fixture::new();
    let before = snapshot(&f.store);
    assert!(
        f.store
            .ownership_transaction(|s| {
                write(s, "early")?;
                s.conn.execute_batch("COMMIT")?;
                Ok(())
            })
            .is_err(),
        "body cannot escape its outer scope"
    );
    assert_eq!(snapshot(&f.store), before);
    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _: Result<()> = f.store.ownership_transaction(|s| {
            write(s, "panic")?;
            panic!("fixture body panic before physical RELEASE");
        });
    }));
    assert!(panic.is_err());
    assert_eq!(snapshot(&f.store), before);
    assert!(f.store.conn.is_autocommit());
    assert!(ownership_latch::exclusive(&f.journal).is_ok());
    f.store
        .ownership_transaction(|s| write(s, "recovered"))
        .unwrap();
}

#[test]
fn unfinished_operation_rolls_back_but_an_existing_outer_transaction_is_not_taken_over() {
    let mut f = Fixture::new();
    let before = snapshot(&f.store);
    assert!(
        f.store
            .ownership_operation(|s| {
                s.conn
                    .execute_batch("BEGIN; INSERT INTO scope_probe VALUES ('unfinished')")?;
                Ok(())
            })
            .is_err()
    );
    assert!(f.store.conn.is_autocommit());
    assert_eq!(snapshot(&f.store), before);
    f.store.conn.execute_batch("BEGIN").unwrap();
    assert!(f.store.ownership_transaction(|_| Ok(())).is_err());
    assert!(
        !f.store.conn.is_autocommit(),
        "caller still owns its transaction"
    );
    f.store.conn.execute_batch("ROLLBACK").unwrap();
}

fn family(store: &mut Store, family: &str, tag: &str) -> Result<()> {
    match family {
        "transact" => {
            write(store, tag)?;
        }
        "batch" => {
            let d = Datum {
                entity: store.intern(&format!("urn:batch:{tag}"))?,
                attribute: store.intern("urn:batch:p")?,
                value: Value::Int(1),
                valid_from: TS.into(),
                valid_to: None,
                op: Op::Assert,
            };
            store.transact_graph_batches_tx(&[(0, vec![d])], TS, None, None)?;
        }
        "registry" => {
            store.load_shapes(tag, SHAPES, TS)?;
        }
        "fork" => {
            store.fork_create(tag, store.latest_tx_id()?, TS, None)?;
        }
        "cursor" => {
            store.commit_consumer(tag, 1, TS)?;
        }
        "subscription" => {
            store.subscription_create(
                tag,
                None,
                None,
                "realtime",
                "https://example.invalid/hook",
                1,
                1,
                TS,
            )?;
        }
        "rdf-import" => {
            let nt = format!("<urn:import:{tag}> <urn:import:p> \"value\" .");
            crate::rdf::ingest_rdf(
                store,
                nt.as_bytes(),
                oxrdfio::RdfFormat::NTriples,
                None,
                TS,
                None,
                None,
            )?;
        }
        _ => unreachable!(),
    }
    Ok(())
}

#[test]
fn actual_store_writer_families_pair_positive_with_retired_refusal() {
    for operation in [
        "transact",
        "batch",
        "registry",
        "fork",
        "cursor",
        "subscription",
        "rdf-import",
    ] {
        let mut f = Fixture::new();
        let empty = snapshot(&f.store);
        f.store
            .ownership_operation(|s| family(s, operation, "before"))
            .unwrap();
        assert_ne!(
            snapshot(&f.store),
            empty,
            "{operation}: actual positive writer"
        );
        super::super::ownership_journal::advance(&f.journal, &f.active(), &f.retiring()).unwrap();
        let before = snapshot(&f.store);
        assert!(
            f.store
                .ownership_operation(|s| family(s, operation, "after"))
                .is_err(),
            "{operation}: scoped"
        );
        assert!(
            family(&mut f.store, operation, "unscoped").is_err(),
            "{operation}: direct"
        );
        assert_eq!(
            snapshot(&f.store),
            before,
            "{operation}: no database residue"
        );
    }
}

#[cfg(feature = "server")]
#[test]
fn wake_callback_replacement_keeps_the_actual_scope_backstop() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let mut f = Fixture::new();
    let wakes = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&wakes);
    f.store
        .set_commit_wake(move || {
            observed.fetch_add(1, Ordering::SeqCst);
        })
        .unwrap();
    f.store
        .ownership_transaction(|s| write(s, "positive-wake"))
        .unwrap();
    assert_eq!(wakes.load(Ordering::SeqCst), 1);
    assert!(
        f.store
            .conn
            .execute("INSERT INTO scope_probe VALUES ('unframed')", [])
            .is_err()
    );
    assert_eq!(wakes.load(Ordering::SeqCst), 1);
    super::super::ownership_journal::advance(&f.journal, &f.active(), &f.retiring()).unwrap();
    let observed = Arc::clone(&wakes);
    f.store
        .set_commit_wake(move || {
            observed.fetch_add(1, Ordering::SeqCst);
        })
        .unwrap();
    assert!(
        f.store
            .ownership_transaction(|s| write(s, "retired-wake"))
            .is_err()
    );
    assert_eq!(wakes.load(Ordering::SeqCst), 1);
}

#[path = "ownership_scope_race_tests.rs"]
mod races;
#[path = "ownership_scope_transfer_tests.rs"]
mod transfers;
