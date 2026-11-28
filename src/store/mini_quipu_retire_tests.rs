//! M5 exit proof: a merge after retirement refuses and writes nothing; a
//! write to a retired mini-Quipu refuses on every path; home history reads
//! the same with the file deleted; a retired entry never reopens. Sabotage:
//! a retirement that leaves the writer unfenced fails the write-refusal check.
use std::path::{Path, PathBuf};

use super::super::local::{MaterializeSpec, MiniQuipu, materialize_on_legacy};
use super::super::merge::{MergeBack, MergeMode, MergeRequest, merge_on_legacy};
use super::super::tests::{FROM, Home, home};
use super::super::{MemberRole, MiniForkMember, MiniForkRequest, mini_fork_entry};
use super::*;
use crate::schema::ROOT_GRAPH_IRI;
use crate::store::Datum;
use crate::types::{Op, Value};

const LATER: &str = "2026-03-01T00:00:00Z";
const TD: &str = "2026-06-01T00:00:00Z";
const TR: &str = "2026-07-01T00:00:00Z";

struct Fixture {
    home: Home,
    mini: Option<MiniQuipu>,
    path: PathBuf,
}

fn datum(store: &Store, v: &str) -> Vec<Datum> {
    vec![Datum {
        entity: store.intern("urn:x").unwrap(),
        attribute: store.intern("urn:many").unwrap(),
        value: Value::Str(v.into()),
        valid_from: LATER.into(),
        valid_to: None,
        op: Op::Assert,
    }]
}

fn fixture(dir: &Path) -> Fixture {
    let mut home = home();
    let path = dir.join("mini.db");
    let spec = MaterializeSpec {
        name: "agent-a".into(),
        members: vec![MiniForkMember {
            iri: ROOT_GRAPH_IRI.into(),
            role: MemberRole::Writable,
        }],
        created_at: FROM.into(),
    };
    let mut mini = materialize_on_legacy(&home.catalog, &mut home.legacy, &spec, &path).unwrap();
    let d = datum(mini.store(), "local");
    mini.transact(ROOT_GRAPH_IRI, &d, LATER, Some("writer"))
        .unwrap();
    Fixture {
        home,
        mini: Some(mini),
        path,
    }
}

fn request() -> MergeRequest<'static> {
    MergeRequest {
        name: "agent-a",
        mode: MergeMode::Atomic,
        decided: None,
        timestamp: TD,
        actor: Some("applier"),
    }
}

fn merged(f: &mut Fixture) -> String {
    let out = merge_on_legacy(&f.home.catalog, &mut f.home.legacy, &f.path, &request()).unwrap();
    let MergeBack::Merged(receipt) = out else {
        panic!("expected a merge: {out:?}");
    };
    receipt.merge_id
}

fn retire_req(reason: RetireReason) -> RetireRequest<'static> {
    RetireRequest {
        name: "agent-a",
        reason,
        timestamp: TR,
    }
}

/// (fact rows, transactions, member-graph rows) — the "nothing written" measure.
fn footprint(store: &Store) -> (i64, i64) {
    let q = |sql: &str| store.conn.query_row(sql, [], |r| r.get(0)).unwrap();
    (
        q("SELECT count(*) FROM facts"),
        q("SELECT count(*) FROM transactions"),
    )
}

/// Every write path on the file refuses: the native wrapper, and a raw
/// write-pipeline write on a plain handle. Neither changes the file.
fn post_retire_write_refuses(path: &Path, entry: &MiniForkEntry) -> bool {
    let mut mini = MiniQuipu::open(path, entry).unwrap();
    let before = footprint(mini.store());
    let d = datum(mini.store(), "after-retire");
    let wrapper = mini.transact(ROOT_GRAPH_IRI, &d, TR, None);
    drop(mini);
    let mut raw = Store::open(path.to_str().unwrap()).unwrap();
    let d = datum(&raw, "after-retire-raw");
    let pipeline = raw.ownership_operation(|s| s.transact_to_graph(&d, TR, None, None, 0));
    let refused = |r: &Result<i64>| {
        r.as_ref()
            .is_err_and(|e| e.to_string().contains("retired-writer-fenced"))
    };
    refused(&wrapper) && refused(&pipeline) && footprint(&raw) == before
}

/// The home history a reader sees: every transaction row, the entity
/// history of the merged subject, and its current facts with their source.
fn history(home: &Store) -> String {
    let mut stmt = home
        .conn
        .prepare("SELECT id, timestamp, actor, source FROM transactions ORDER BY id")
        .unwrap();
    let txs: Vec<String> = stmt
        .query_map([], |r| {
            Ok(format!(
                "{}|{}|{:?}|{:?}",
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, Option<String>>(3)?
            ))
        })
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    let x = home.intern("urn:x").unwrap();
    let facts: Vec<String> = home
        .entity_history(x)
        .unwrap()
        .iter()
        .map(|f| {
            let src: Option<String> = home
                .conn
                .query_row("SELECT source FROM transactions WHERE id=?1", [f.tx], |r| {
                    r.get(0)
                })
                .unwrap();
            format!("{f:?} src={src:?}")
        })
        .collect();
    format!("{txs:#?}\n{facts:#?}")
}

#[test]
fn a_merge_after_retirement_refuses_with_nothing_written() {
    let dir = tempfile::tempdir().unwrap();
    let mut f = fixture(dir.path());
    let merge_id = merged(&mut f);
    let r = retire_on_legacy(
        &mut f.home.legacy,
        &f.path,
        &retire_req(RetireReason::FinalMerge),
    )
    .unwrap();
    assert_eq!(r.last_merge, merge_id);
    assert_eq!(retirement(&f.home.legacy, "agent-a").unwrap(), Some(r));
    let before = footprint(&f.home.legacy);
    let err =
        merge_on_legacy(&f.home.catalog, &mut f.home.legacy, &f.path, &request()).unwrap_err();
    assert!(err.to_string().contains("registry-closed"), "{err}");
    assert_eq!(footprint(&f.home.legacy), before, "a retired merge wrote");
}

#[test]
fn retirement_writes_nothing_to_home_member_graphs() {
    let dir = tempfile::tempdir().unwrap();
    let mut f = fixture(dir.path());
    let before = footprint(&f.home.legacy);
    let r = retire_on_legacy(
        &mut f.home.legacy,
        &f.path,
        &retire_req(RetireReason::Abandoned),
    )
    .unwrap();
    assert_eq!(r.last_merge, "abandoned");
    assert_eq!(
        footprint(&f.home.legacy),
        before,
        "retirement wrote history"
    );
    let status: String = f
        .home
        .legacy
        .conn
        .query_row(
            "SELECT status FROM mini_quipu_forks WHERE name='agent-a'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(status, "retired");
}

#[test]
fn final_merge_retirement_with_unmerged_local_writes_refuses_with_nothing_written() {
    let dir = tempfile::tempdir().unwrap();
    let mut f = fixture(dir.path());
    let err = retire_on_legacy(
        &mut f.home.legacy,
        &f.path,
        &retire_req(RetireReason::FinalMerge),
    )
    .unwrap_err();
    assert!(err.to_string().contains("unmerged-local-writes"), "{err}");
    merged(&mut f);
    let mini = f.mini.as_mut().unwrap();
    let d = datum(mini.store(), "later");
    mini.transact(ROOT_GRAPH_IRI, &d, TD, None).unwrap();
    let before = footprint(&f.home.legacy);
    let err = retire_on_legacy(
        &mut f.home.legacy,
        &f.path,
        &retire_req(RetireReason::FinalMerge),
    )
    .unwrap_err();
    assert!(err.to_string().contains("unmerged-local-writes"), "{err}");
    assert_eq!(footprint(&f.home.legacy), before);
    assert!(fenced(f.mini.as_ref().unwrap().store()).unwrap().is_none());
    assert!(retirement(&f.home.legacy, "agent-a").unwrap().is_none());
}

#[test]
fn a_write_to_a_retired_mini_quipu_refuses_on_every_path() {
    let dir = tempfile::tempdir().unwrap();
    let mut f = fixture(dir.path());
    merged(&mut f);
    f.mini = None;
    retire_on_legacy(
        &mut f.home.legacy,
        &f.path,
        &retire_req(RetireReason::FinalMerge),
    )
    .unwrap();
    let entry = mini_fork_entry(&f.home.legacy, "agent-a").unwrap().unwrap();
    assert!(post_retire_write_refuses(&f.path, &entry));
    let raw = Store::open(f.path.to_str().unwrap()).unwrap();
    assert!(
        raw.conn
            .execute("DELETE FROM mini_quipu_fence", [])
            .is_err()
    );
    assert!(raw.conn.execute("DELETE FROM facts", []).is_err());
}

#[test]
fn sabotage_an_unfenced_retirement_fails_the_write_refusal_check() {
    let dir = tempfile::tempdir().unwrap();
    let mut f = fixture(dir.path());
    merged(&mut f);
    f.mini = None;
    retire(
        &mut f.home.legacy,
        &f.path,
        &retire_req(RetireReason::FinalMerge),
        |_, _| Ok(()),
    )
    .unwrap();
    let entry = mini_fork_entry(&f.home.legacy, "agent-a").unwrap().unwrap();
    assert!(
        !post_retire_write_refuses(&f.path, &entry),
        "an unfenced writer passed the post-retire write refusal check"
    );
}

#[test]
fn home_history_reads_the_same_with_the_mini_quipu_file_deleted() {
    let dir = tempfile::tempdir().unwrap();
    let mut f = fixture(dir.path());
    let merge_id = merged(&mut f);
    f.mini = None;
    retire_on_legacy(
        &mut f.home.legacy,
        &f.path,
        &retire_req(RetireReason::FinalMerge),
    )
    .unwrap();
    let receipts = verify_receipts(&f.home.legacy, "agent-a").unwrap();
    assert_eq!(receipts.len(), 1);
    assert_eq!(receipts[0].merge_id, merge_id);
    let before = history(&f.home.legacy);
    assert!(
        before.contains("writer") || before.contains(&merge_id),
        "{before}"
    );
    assert!(before.contains(&format!("merge={merge_id}")), "{before}");
    for suffix in ["", "-wal", "-shm"] {
        let mut p = f.path.as_os_str().to_owned();
        p.push(suffix);
        let _ = std::fs::remove_file(p);
    }
    assert!(!f.path.exists());
    assert_eq!(history(&f.home.legacy), before);
    assert_eq!(
        verify_receipts(&f.home.legacy, "agent-a").unwrap(),
        receipts
    );
}

#[test]
fn a_tampered_receipt_does_not_verify() {
    let dir = tempfile::tempdir().unwrap();
    let mut f = fixture(dir.path());
    merged(&mut f);
    f.home
        .legacy
        .conn
        .execute(
            "UPDATE transactions SET source='other' WHERE source LIKE 'mini-quipu-merge:%'",
            [],
        )
        .unwrap();
    let err = verify_receipts(&f.home.legacy, "agent-a").unwrap_err();
    assert!(err.to_string().contains("receipt unverified"), "{err}");
}

#[test]
fn reopening_a_retired_entry_refuses() {
    let dir = tempfile::tempdir().unwrap();
    let mut f = fixture(dir.path());
    let entry = mini_fork_entry(&f.home.legacy, "agent-a").unwrap().unwrap();
    retire_on_legacy(
        &mut f.home.legacy,
        &f.path,
        &retire_req(RetireReason::Abandoned),
    )
    .unwrap();
    for sql in [
        "UPDATE mini_quipu_forks SET status='open' WHERE name='agent-a'",
        "DELETE FROM mini_quipu_forks WHERE name='agent-a'",
        "DELETE FROM mini_quipu_retirements WHERE name='agent-a'",
    ] {
        let err = f.home.legacy.conn.execute(sql, []).unwrap_err();
        assert!(err.to_string().contains("retired-never-reopens"), "{err}");
    }
    let before = footprint(&f.home.legacy);
    let err = retire_on_legacy(
        &mut f.home.legacy,
        &f.path,
        &retire_req(RetireReason::Abandoned),
    )
    .unwrap_err();
    assert!(err.to_string().contains("retired-never-reopens"), "{err}");
    let again = MiniForkRequest {
        name: "agent-a".into(),
        store_id: entry.store_id.clone(),
        log_epoch: entry.log_epoch.clone(),
        members: vec![MiniForkMember {
            iri: ROOT_GRAPH_IRI.into(),
            role: MemberRole::Writable,
        }],
        created_at: TR.into(),
    };
    assert!(super::super::record_on_legacy(&f.home.catalog, &mut f.home.legacy, &again).is_err());
    assert_eq!(footprint(&f.home.legacy), before);
    let status: String = f
        .home
        .legacy
        .conn
        .query_row(
            "SELECT status FROM mini_quipu_forks WHERE name='agent-a'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(status, "retired");
}

#[test]
fn review_final_merge_must_not_strand_a_write_between_head_check_and_fence() {
    let dir = tempfile::tempdir().unwrap();
    let mut f = fixture(dir.path());
    merged(&mut f);
    f.mini = None;
    let path = f.path.clone();
    let entry = mini_fork_entry(&f.home.legacy, "agent-a").unwrap().unwrap();
    let outcome = retire(
        &mut f.home.legacy,
        &f.path,
        &retire_req(RetireReason::FinalMerge),
        |mini, retirement| {
            let mut racer = MiniQuipu::open(&path, &entry)?;
            let data = datum(racer.store(), "racing-local");
            racer.transact(ROOT_GRAPH_IRI, &data, TR, Some("concurrent-writer"))?;
            assert!(racer.store().latest_tx_id()? > retirement.fenced_head);
            fence_writer(mini, retirement)
        },
    );
    let raw = Store::open(path.to_str().unwrap()).unwrap();
    let local = format!(
        "{:?}",
        raw.entity_history(raw.intern("urn:x").unwrap()).unwrap()
    );
    assert!(local.contains("racing-local"), "racer must have committed");
    assert!(
        !history(&f.home.legacy).contains("racing-local"),
        "racing work was not merged home"
    );
    assert!(
        outcome.is_err(),
        "FinalMerge retired after a local write landed between its head check and writer fence: outcome={outcome:?}, actual_head={:?}, fence={:?}, registry={:?}",
        raw.latest_tx_id(),
        fenced(&raw),
        retirement(&f.home.legacy, "agent-a")
    );
    assert!(
        outcome
            .unwrap_err()
            .to_string()
            .contains("unmerged-local-writes")
    );
    assert!(fenced(&raw).unwrap().is_none());
    assert!(retirement(&f.home.legacy, "agent-a").unwrap().is_none());
    assert_eq!(
        mini_fork_entry(&f.home.legacy, "agent-a").unwrap().unwrap(),
        entry
    );
    assert!(!has_table(&raw, "mini_quipu_fence").unwrap());
}
