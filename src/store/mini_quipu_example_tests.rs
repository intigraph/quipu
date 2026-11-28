//! M6: one generic two-fork lifecycle, repeated under renamed graph IRIs.
use super::super::local::{MaterializeSpec, MiniQuipu, StoreCursor};
use super::super::merge::{MergeBack, MergeMode, MergeReceipt, MergeRequest};
use super::super::retire::{RetireReason, RetireRequest, verify_receipts};
use super::*;
use crate::store::forks_merge::ForkResolutions;

const SHAPES: &str = "@prefix sh: <http://www.w3.org/ns/shacl#> .
<urn:example-shape> a sh:NodeShape ; sh:targetSubjectsOf <urn:value> ;
sh:property [ sh:path <urn:value> ; sh:maxCount 1 ] .";
const LOCAL: &str = "2026-02-01T00:00:00Z";
const MERGE: &str = "2026-04-01T00:00:00Z";
const RETIRE: &str = "2026-05-01T00:00:00Z";

fn renamed_home(graphs: [&str; 3]) -> Home {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let mut store = adopted(root, "application.db", &[graphs[0], graphs[1]]);
    store
        .ownership_operation(|s| s.load_shapes("example-shapes", SHAPES, FROM))
        .unwrap();
    let mut legacy = adopted(root, "legacy.db", &[graphs[2]]);
    let datum = |s: &Store, v: &str| Datum {
        entity: s.intern("urn:item").unwrap(),
        attribute: s.intern("urn:value").unwrap(),
        value: TermValue::Str(v.into()),
        valid_from: FROM.into(),
        valid_to: None,
        op: Op::Assert,
    };
    let leg_g = legacy.committed_graph_id(graphs[2]).unwrap().unwrap();
    let leg_datum = datum(&legacy, "noted");
    legacy
        .ownership_operation(|s| {
            s.transact_graph_batches_tx(&[(leg_g, vec![leg_datum])], FROM, None, None)
        })
        .unwrap();
    let (routed, legacy_id) = (
        route(graphs[0], "application", &store, 3),
        legacy.store_id().unwrap(),
    );
    let batch = GraphSetBatch {
        store_id: routed.store_id.clone(),
        log_epoch: routed.log_epoch.clone(),
        graphs: vec![
            (graphs[0].into(), vec![datum(&store, "present")]),
            (graphs[1].into(), vec![datum(&store, "recorded")]),
        ],
        timestamp: FROM.into(),
        actor: Some("fixture".into()),
        source: Some("fixture".into()),
    };
    let catalog = Arc::new(
        GraphSetCatalog::new(
            7,
            &legacy_id,
            vec![GraphSet {
                owner: routed.clone(),
                graphs: vec![graphs[0].into(), graphs[1].into()],
            }],
            Budget {
                max_owners: 1,
                queued_per_owner: 1,
                readers_per_owner: 1,
                max_queued: 1,
                max_connections: 8,
            },
            8,
        )
        .unwrap()
        .with_legacy_graphs(
            route(ROOT_GRAPH_IRI, "legacy", &legacy, 1),
            vec![ROOT_GRAPH_IRI.into(), graphs[2].into()],
        )
        .unwrap(),
    );
    let allocation = catalog.physical().memory_allocation(512, 3, 1).unwrap();
    let owner = GraphSetOwner::prepare(
        catalog.clone(),
        root,
        graphs[0],
        store,
        "policy-v1",
        allocation,
    )
    .unwrap();
    let requests: Vec<_> = [graphs[0], graphs[1]]
        .into_iter()
        .map(|graph| WriteRequest {
            graph: Some(graph),
            catalog_revision: 7,
            owner_generation: Some(3),
            policy: "policy-v1",
            signed: false,
            cross_graph_context: false,
        })
        .collect();
    owner.transact(&requests, &batch).unwrap();
    Home {
        _dir: dir,
        owner,
        legacy,
        catalog,
        routed_id: routed.store_id,
        legacy_id,
    }
}

fn spec(name: &str, graphs: &[&str]) -> MaterializeSpec {
    MaterializeSpec {
        name: name.into(),
        members: graphs
            .iter()
            .map(|g| MiniForkMember {
                iri: (*g).into(),
                role: MemberRole::Writable,
            })
            .collect(),
        created_at: FROM.into(),
    }
}

fn change(mini: &mut MiniQuipu, graph: &str, value: &str, actor: &str) -> i64 {
    let s = mini.store();
    let datum = |value: &str, op| Datum {
        entity: s.intern("urn:item").unwrap(),
        attribute: s.intern("urn:value").unwrap(),
        value: TermValue::Str(value.into()),
        valid_from: LOCAL.into(),
        valid_to: None,
        op,
    };
    let ds = vec![datum("present", Op::Retract), datum(value, Op::Assert)];
    mini.transact(graph, &ds, LOCAL, Some(actor)).unwrap()
}

fn request<'a>(name: &'a str, resolutions: Option<&'a ForkResolutions>) -> MergeRequest<'a> {
    MergeRequest {
        name,
        mode: MergeMode::Atomic,
        decided: resolutions.map(|r| (r, "example-resolution", "reviewer")),
        timestamp: MERGE,
        actor: Some("applier"),
    }
}

fn merged(out: MergeBack) -> MergeReceipt {
    let MergeBack::Merged(receipt) = out else {
        panic!("expected merged: {out:?}")
    };
    assert_eq!(receipt.held, 0);
    receipt
}

fn values(s: &Store, graph: &str) -> Vec<String> {
    let g = s.committed_graph_id(graph).unwrap().unwrap();
    s.current_facts_in_graph(g)
        .unwrap()
        .iter()
        .map(|f| match &f.value {
            TermValue::Str(v) => v.clone(),
            other => panic!("unexpected {other:?}"),
        })
        .collect()
}

fn worked_example(graphs: [&str; 3]) {
    let home = renamed_home(graphs);
    let root = home._dir.path();
    let head = || home.owner.owner.read(Store::latest_tx_id).unwrap();
    let before = head();
    // B1: a partial catalog set refuses without a file, entry or home write.
    let partial = root.join("partial.db");
    let err = home
        .owner
        .materialize_mini_quipu(&spec("partial", &graphs[..1]), &partial)
        .err()
        .unwrap()
        .to_string();
    assert!(err.contains("X2"), "{err}");
    assert!(!partial.exists());
    assert_eq!(home.owner.owner.read(mini_fork_count).unwrap(), 0);
    assert_eq!(head(), before);

    let paths = [root.join("a.db"), root.join("b.db")];
    let mut a = home
        .owner
        .materialize_mini_quipu(&spec("a", &graphs[..2]), &paths[0])
        .unwrap();
    let mut b = home
        .owner
        .materialize_mini_quipu(&spec("b", &graphs[..2]), &paths[1])
        .unwrap();
    assert_eq!(a.entry().base, b.entry().base);
    assert_ne!(a.entry().store_id, b.entry().store_id);
    let tx_a = change(&mut a, graphs[0], "alice", "writer-a");
    let tx_b = change(&mut b, graphs[0], "bob", "writer-b");

    // B3: discovery ranges only over the declared members; local claims stay local.
    let found = crate::sparql::query(
        a.store(),
        "SELECT DISTINCT ?g WHERE { GRAPH ?g { <urn:item> <urn:value> ?v } }",
    )
    .unwrap();
    let discovered: std::collections::BTreeSet<_> = found
        .rows()
        .iter()
        .map(|row| {
            let TermValue::Ref(id) = row["g"] else {
                panic!("graph binding is not an IRI")
            };
            a.store().resolve(id).unwrap()
        })
        .collect();
    assert_eq!(
        discovered,
        graphs[..2].iter().map(|g| (*g).to_owned()).collect()
    );
    assert_eq!(
        home.owner.owner.read(|s| Ok(values(s, graphs[0]))).unwrap(),
        ["present"]
    );
    assert_eq!(values(a.store(), graphs[0]), ["alice"]);
    assert_eq!(values(b.store(), graphs[0]), ["bob"]);
    assert_eq!(values(&home.legacy, graphs[2]), ["noted"]);

    // B4: positions are not transferable between home and either fresh lineage.
    let home_cursor = home.owner.owner.read(StoreCursor::head).unwrap();
    let local_cursor = StoreCursor::head(a.store()).unwrap();
    assert!(
        home_cursor
            .admit(a.store())
            .err()
            .unwrap()
            .to_string()
            .contains("X8")
    );
    assert!(
        home.owner
            .owner
            .read(|s| local_cursor.admit(s))
            .err()
            .unwrap()
            .to_string()
            .contains("X8")
    );
    assert!(local_cursor.admit(b.store()).is_err());

    let first = merged(
        home.owner
            .merge_mini_quipu(&paths[0], &request("a", None))
            .unwrap(),
    );
    let after_first = head();
    let held = home
        .owner
        .merge_mini_quipu(&paths[1], &request("b", None))
        .unwrap();
    assert!(
        matches!(held, MergeBack::Conflicts(ref cs) if cs.len() == 1),
        "{held:?}"
    );
    assert_eq!(head(), after_first, "atomic conflict wrote");
    assert_eq!(
        home.owner.owner.read(|s| Ok(values(s, graphs[0]))).unwrap(),
        ["alice"]
    );
    let key = home
        .owner
        .owner
        .read(|s| {
            Ok((
                s.lookup("urn:item")?.unwrap(),
                s.lookup("urn:value")?.unwrap(),
            ))
        })
        .unwrap();
    // A decision for no member must still refuse, rather than disappear in scoping.
    let invalid = ForkResolutions::from([((key.0, key.0), vec![TermValue::Str("bogus".into())])]);
    let err = home
        .owner
        .merge_mini_quipu(&paths[1], &request("b", Some(&invalid)))
        .err()
        .unwrap()
        .to_string();
    assert!(err.contains("not in conflict"), "{err}");
    assert_eq!(head(), after_first, "invalid decision wrote");
    let res = ForkResolutions::from([(key, vec![TermValue::Str("bob".into())])]);
    let second = merged(
        home.owner
            .merge_mini_quipu(&paths[1], &request("b", Some(&res)))
            .unwrap(),
    );
    assert_eq!(
        home.owner.owner.read(|s| Ok(values(s, graphs[0]))).unwrap(),
        ["bob"]
    );
    assert!(!second.decide_tx.is_empty());

    // B7: the merge carries observed origin data and never invents declarations.
    for (receipt, tx, actor) in [(&first, tx_a, "writer-a"), (&second, tx_b, "writer-b")] {
        assert!(receipt.rows.iter().any(|r| r.origin_txs.contains(&tx)));
        let carried = receipt.carried.as_ref().unwrap();
        assert!(
            carried
                .transactions
                .values()
                .any(|c| c.actor.as_deref() == Some(actor))
        );
        assert!(
            carried
                .transactions
                .values()
                .all(|c| c.declarations.is_none())
        );
        for tx in receipt
            .receive_tx
            .iter()
            .chain(&receipt.decide_tx)
            .filter(|tx| **tx != 0)
        {
            let actor: Option<String> = home
                .owner
                .owner
                .read(|s| {
                    Ok(s.conn.query_row(
                        "SELECT actor FROM transactions WHERE id=?1",
                        [tx],
                        |r| r.get(0),
                    )?)
                })
                .unwrap();
            assert!(matches!(actor.as_deref(), Some("applier" | "reviewer")));
        }
    }
    let before_retry = head();
    assert!(
        matches!(home.owner.merge_mini_quipu(&paths[1], &request("b", Some(&res))).unwrap(),
        MergeBack::AlreadyApplied(ref r) if r == &second)
    );
    assert_eq!(head(), before_retry);

    drop(a);
    drop(b);
    for (name, path) in ["a", "b"].into_iter().zip(&paths) {
        let retired = home
            .owner
            .retire_mini_quipu(
                path,
                &RetireRequest {
                    name,
                    reason: RetireReason::FinalMerge,
                    timestamp: RETIRE,
                },
            )
            .unwrap();
        let receipts = home.owner.owner.read(|s| verify_receipts(s, name)).unwrap();
        assert!(receipts.iter().any(|r| r.merge_id == retired.last_merge));
        assert!(
            home.owner
                .merge_mini_quipu(path, &request(name, None))
                .is_err()
        );
        let entry = home.owner.mini_fork(name).unwrap().unwrap();
        let mut local = MiniQuipu::open(path, &entry).unwrap();
        let d = Datum {
            entity: local.store().intern("urn:after").unwrap(),
            attribute: local.store().intern("urn:value").unwrap(),
            value: TermValue::Str("late".into()),
            valid_from: RETIRE.into(),
            valid_to: None,
            op: Op::Assert,
        };
        assert!(local.transact(graphs[0], &[d], RETIRE, None).is_err());
        drop(local);
        std::fs::remove_file(path).unwrap();
    }
    assert_eq!(head(), before_retry, "retirement changed member history");
    assert_eq!(
        home.owner.owner.read(|s| Ok(values(s, graphs[0]))).unwrap(),
        ["bob"]
    );
}

#[test]
fn m6_generic_worked_example_with_original_graphs() {
    worked_example([
        "urn:example:work",
        "urn:example:evidence",
        "urn:example:outside",
    ]);
}

#[test]
fn m6_generic_worked_example_with_renamed_graphs() {
    worked_example([
        "https://example.org/x",
        "https://example.org/y",
        "https://example.org/z",
    ]);
}
