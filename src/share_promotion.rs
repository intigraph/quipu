//! Snapshot promotion must not undo a later local decision to retract a fact.
//!
//! A foreign `tx_anchor` is not comparable with the receiving store's transaction
//! IDs. Conservatively retain ROOT's exact-fact retractions until the operator
//! explicitly reasserts that fact locally. Source graph membership remains the
//! unmodified evidence of what the pack supplied.

use std::collections::HashSet;

use crate::error::Result;
use crate::store::{Datum, Store};
use crate::types::{Fact, Op};

pub(crate) fn promote_snapshot(
    store: &mut Store,
    graph: i64,
    timestamp: &str,
    actor: Option<&str>,
    source: &str,
) -> Result<(i64, usize, usize)> {
    store
        .conn
        .execute_batch("SAVEPOINT quipu_snapshot_promotion")?;
    let result = (|| {
        let facts = store.current_facts_in_graph(graph)?;
        let (datums, suppressed) = without_retracted_root_facts(store, &facts, timestamp)?;
        let tx = store.transact(&datums, timestamp, actor, Some(source))?;
        Ok((tx, datums.len(), suppressed))
    })();
    match result {
        Ok(value) => {
            store
                .conn
                .execute_batch("RELEASE quipu_snapshot_promotion")?;
            Ok(value)
        }
        Err(error) => {
            store.conn.execute_batch(
                "ROLLBACK TO quipu_snapshot_promotion; RELEASE quipu_snapshot_promotion",
            )?;
            store.read_model.borrow_mut().clear();
            Err(error)
        }
    }
}

/// What a subject-scoped promotion changed.
#[derive(Debug, Clone, Default)]
pub(crate) struct SubjectMove {
    /// 0 when nothing needed to change (an idempotent repeat).
    pub tx_id: i64,
    pub asserted: usize,
    pub replaced: usize,
    pub retired: usize,
    pub retire_from: Option<String>,
}

/// Promote a staged share into the named graph `target`, as a MOVE of exactly
/// the share's subjects (aegis-27l: rb3 B3, tg5 F5):
///
/// - in `target`, each staged subject ends up holding exactly its staged facts;
///   facts of every other subject in `target` are untouched (this is not
///   [`crate::project_graph::promote_into_graph`]'s whole-graph diff, which
///   retracts whatever the share does not mention);
/// - with `retire_from`, the same subjects' current facts are retracted from that
///   graph; its other subjects are untouched.
///
/// One transaction per graph, both inside the caller's savepoint, so a failure
/// or an expired request deadline rolls back the move whole. The writer is
/// bounded the way /import is (aegis-oa5): `check_request_deadline` between the
/// steps, never a partial state to resume. A share too big for one budget is
/// split by the caller into smaller shares.
pub(crate) fn promote_subjects_into_graph(
    store: &mut Store,
    staging: i64,
    target: &str,
    retire_from: Option<&str>,
    timestamp: &str,
    actor: Option<&str>,
    source: &str,
) -> Result<SubjectMove> {
    if retire_from == Some(target) {
        return Err(crate::Error::InvalidValue(
            "retire_from must differ from target_graph: a graph cannot be moved into itself".into(),
        ));
    }
    let wanted = store.current_facts_in_graph(staging)?;
    let subjects: HashSet<i64> = wanted.iter().map(|f| f.entity).collect();
    let key = |f: &Fact| (f.entity, f.attribute, f.value.to_bytes());
    let datum = |f: &Fact, op| Datum {
        entity: f.entity,
        attribute: f.attribute,
        value: f.value.clone(),
        valid_from: timestamp.to_string(),
        valid_to: None,
        op,
    };
    crate::time::check_request_deadline()?;
    let g = store.graph_create(target)?;
    let have: Vec<Fact> = store
        .current_facts_in_graph(g)?
        .into_iter()
        .filter(|f| subjects.contains(&f.entity))
        .collect();
    let wanted_keys: HashSet<_> = wanted.iter().map(key).collect();
    let have_keys: HashSet<_> = have.iter().map(key).collect();
    let mut changes: Vec<Datum> = have
        .iter()
        .filter(|f| !wanted_keys.contains(&key(f)))
        .map(|f| datum(f, Op::Retract))
        .collect();
    let replaced = changes.len();
    changes.extend(
        wanted
            .iter()
            .filter(|f| !have_keys.contains(&key(f)))
            .map(|f| datum(f, Op::Assert)),
    );
    let asserted = changes.len() - replaced;
    let mut moved = SubjectMove {
        asserted,
        replaced,
        retire_from: retire_from.map(String::from),
        ..SubjectMove::default()
    };
    crate::time::check_request_deadline()?;
    if !changes.is_empty() {
        moved.tx_id = store.transact_to_graph(&changes, timestamp, actor, Some(source), g)?;
    }
    if let Some(from) = retire_from {
        // An unknown source graph holds nothing to retire; never create one.
        if let Some(b) = store.lookup(from)? {
            crate::time::check_request_deadline()?;
            let retire: Vec<Datum> = store
                .current_facts_in_graph(b)?
                .iter()
                .filter(|f| subjects.contains(&f.entity))
                .map(|f| datum(f, Op::Retract))
                .collect();
            moved.retired = retire.len();
            if !retire.is_empty() {
                let tx = store.transact_to_graph(&retire, timestamp, actor, Some(source), b)?;
                moved.tx_id = moved.tx_id.max(tx);
            }
        }
    }
    Ok(moved)
}

fn without_retracted_root_facts(
    store: &Store,
    facts: &[Fact],
    timestamp: &str,
) -> Result<(Vec<Datum>, usize)> {
    let mut statement = store.conn.prepare(
        "SELECT DISTINCT e, a, v FROM facts WHERE g=0 \
         AND (op=0 OR retracted_tx IS NOT NULL)",
    )?;
    let mut retracted: HashSet<(i64, i64, Vec<u8>)> = statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
        .collect::<std::result::Result<_, _>>()?;
    // A deliberate local reassertion supersedes a historical retraction. An
    // unrelated predicate, value, or graph never suppresses this fact.
    for fact in store.current_facts()? {
        retracted.remove(&(fact.entity, fact.attribute, fact.value.to_bytes()));
    }
    let datums: Vec<_> = facts
        .iter()
        .filter(|fact| !retracted.contains(&(fact.entity, fact.attribute, fact.value.to_bytes())))
        .map(|fact| Datum {
            entity: fact.entity,
            attribute: fact.attribute,
            value: fact.value.clone(),
            valid_from: timestamp.to_string(),
            valid_to: None,
            op: Op::Assert,
        })
        .collect();
    let suppressed = facts.len() - datums.len();
    Ok((datums, suppressed))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::share_import::{PromoteImportRequest, promote_import};
    use crate::types::Value;

    const TS: &str = "2026-09-24T00:00:00Z";
    const SHARE: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    fn setup() -> (Store, i64, i64, i64, PromoteImportRequest) {
        let mut store = Store::open_in_memory().unwrap();
        let graph = store
            .graph_create(&format!("urn:quipu:import:staging:{}", &SHARE[7..]))
            .unwrap();
        let e = store.intern("https://example.org/entity").unwrap();
        let a = store.intern("https://example.org/value").unwrap();
        let datums = ["old", "different"].map(|v| Datum {
            entity: e,
            attribute: a,
            value: Value::Str(v.into()),
            valid_from: TS.into(),
            valid_to: None,
            op: Op::Assert,
        });
        store
            .transact_to_graph(&datums, TS, None, Some("pack"), graph)
            .unwrap();
        (
            store,
            graph,
            e,
            a,
            PromoteImportRequest {
                share_id: SHARE.into(),
                actor: None,
                target_graph: None,
                retire_from: None,
            },
        )
    }

    #[test]
    fn stale_promotion_preserves_retraction_and_source_membership() {
        let (mut store, graph, e, a, request) = setup();
        promote_import(&mut store, &request, TS, None).unwrap();
        store
            .retract_triples(
                e,
                Some(a),
                Some(&Value::Str("old".into())),
                TS,
                None,
                true,
                None,
            )
            .unwrap();
        for _ in 0..2 {
            let result = promote_import(&mut store, &request, TS, None).unwrap();
            assert_eq!(result.suppressed_retractions, 1);
            let root = store.current_facts().unwrap();
            assert_eq!(root.len(), 1);
            assert_eq!(root[0].value, Value::Str("different".into()));
            assert_eq!(store.current_facts_in_graph(graph).unwrap().len(), 2);
        }
        // A local, explicit correction can restore a fact; pack reload cannot.
        store
            .transact(
                &[Datum {
                    entity: e,
                    attribute: a,
                    value: Value::Str("old".into()),
                    valid_from: TS.into(),
                    valid_to: None,
                    op: Op::Assert,
                }],
                TS,
                None,
                Some("local correction"),
            )
            .unwrap();
        let result = promote_import(&mut store, &request, TS, None).unwrap();
        assert_eq!(result.suppressed_retractions, 0);
        assert_eq!(store.current_facts().unwrap().len(), 2);
    }

    #[test]
    fn another_graphs_retraction_does_not_hide_root_content() {
        let (mut store, _, e, a, request) = setup();
        let other = store.graph_create("urn:other").unwrap();
        let mut datum = Datum {
            entity: e,
            attribute: a,
            value: Value::Str("old".into()),
            valid_from: TS.into(),
            valid_to: None,
            op: Op::Assert,
        };
        store
            .transact_to_graph(&[datum.clone()], TS, None, None, other)
            .unwrap();
        datum.op = Op::Retract;
        store
            .transact_to_graph(&[datum], TS, None, None, other)
            .unwrap();
        let result = promote_import(&mut store, &request, TS, None).unwrap();
        assert_eq!(result.suppressed_retractions, 0);
        assert_eq!(store.current_facts().unwrap().len(), 2);
    }

    // aegis-27l / tg5 F5 counterexample: board B = {A, W}, inbox I = {U}.
    // Moving the share holding A must give B = {W} and I = {A, U}.
    const BOARD: &str = "https://tracker.example/board";
    const INBOX: &str = "https://tracker.example/board/inbox";

    fn subject(store: &mut Store, id: &str) -> i64 {
        store
            .intern(&format!("https://seeds.local/item/{id}"))
            .unwrap()
    }

    fn fact(store: &mut Store, entity: i64, v: &str) -> Datum {
        let a = store.intern("https://example.org/title").unwrap();
        Datum {
            entity,
            attribute: a,
            value: Value::Str(v.into()),
            valid_from: TS.into(),
            valid_to: None,
            op: Op::Assert,
        }
    }

    /// Store with B={A,W}, I={U} and a committed staging share holding A.
    fn move_setup() -> (Store, PromoteImportRequest, [i64; 3]) {
        let mut store = Store::open_in_memory().unwrap();
        let (a, w, u) = (
            subject(&mut store, "aegis-a"),
            subject(&mut store, "aegis-work"),
            subject(&mut store, "messages-b"),
        );
        let board = store.graph_create(BOARD).unwrap();
        let inbox = store.graph_create(INBOX).unwrap();
        let staging = store
            .graph_create(&format!("urn:quipu:import:staging:{}", &SHARE[7..]))
            .unwrap();
        let (fa, fw, fu) = (
            fact(&mut store, a, "A"),
            fact(&mut store, w, "W"),
            fact(&mut store, u, "U"),
        );
        store
            .transact_to_graph(&[fa.clone(), fw], TS, None, Some("board"), board)
            .unwrap();
        store
            .transact_to_graph(&[fu], TS, None, Some("inbox"), inbox)
            .unwrap();
        store
            .transact_to_graph(&[fa], TS, None, Some("pack"), staging)
            .unwrap();
        let request = PromoteImportRequest {
            share_id: SHARE.into(),
            actor: None,
            target_graph: Some(INBOX.into()),
            retire_from: Some(BOARD.into()),
        };
        (store, request, [a, w, u])
    }

    fn subjects_in(store: &Store, iri: &str) -> Vec<i64> {
        let g = store.lookup(iri).unwrap().unwrap();
        let mut s: Vec<i64> = store
            .current_facts_in_graph(g)
            .unwrap()
            .iter()
            .map(|f| f.entity)
            .collect();
        s.sort_unstable();
        s.dedup();
        s
    }

    fn sorted(mut v: Vec<i64>) -> Vec<i64> {
        v.sort_unstable();
        v
    }

    #[test]
    fn subject_move_retires_only_the_moved_subjects() {
        let (mut store, request, [a, w, u]) = move_setup();
        let result = promote_import(&mut store, &request, TS, None).unwrap();
        assert_eq!(result.outcome, "promoted");
        assert_eq!(result.target_graph.as_deref(), Some(INBOX));
        assert_eq!(result.retired, Some(1));
        assert_eq!(subjects_in(&store, BOARD), vec![w]);
        assert_eq!(subjects_in(&store, INBOX), sorted(vec![a, u]));
        // ROOT is untouched by a target promotion.
        assert!(store.current_facts().unwrap().is_empty());
    }

    #[test]
    fn control_whole_graph_promote_drops_the_unrelated_subject() {
        // The pre-27l named-graph promote reproduces the reported wrong result:
        // I loses U, and B still holds A.
        let (mut store, _request, [a, w, u]) = move_setup();
        let staging = format!("urn:quipu:import:staging:{}", &SHARE[7..]);
        crate::project_graph::promote_into_graph(&mut store, SHARE, &staging, INBOX, TS, None)
            .unwrap();
        assert_eq!(subjects_in(&store, INBOX), vec![a]);
        assert_eq!(subjects_in(&store, BOARD), sorted(vec![a, w]));
        let _ = u;
    }

    #[test]
    fn subject_move_repeats_as_unchanged() {
        let (mut store, request, [_, w, _]) = move_setup();
        promote_import(&mut store, &request, TS, None).unwrap();
        let again = crate::share_import::promote_inner(&mut store, &request, TS, None).unwrap();
        assert_eq!(again.outcome, "unchanged");
        assert_eq!(again.tx_id, 0);
        assert_eq!(subjects_in(&store, BOARD), vec![w]);
    }

    #[test]
    fn subject_move_refuses_moving_a_graph_into_itself_and_retire_without_target() {
        let (mut store, mut request, _) = move_setup();
        request.retire_from = Some(INBOX.into());
        assert!(promote_import(&mut store, &request, TS, None).is_err());
        request.target_graph = None;
        request.retire_from = Some(BOARD.into());
        assert!(promote_import(&mut store, &request, TS, None).is_err());
    }

    #[test]
    fn expired_request_deadline_rolls_back_the_whole_move() {
        let (mut store, request, [a, w, u]) = move_setup();
        let expired = crate::time::Deadline::after_millis(0);
        std::thread::sleep(std::time::Duration::from_millis(2));
        let _guard = crate::time::set_request_deadline(Some(expired));
        assert!(promote_import(&mut store, &request, TS, None).is_err());
        drop(_guard);
        assert_eq!(subjects_in(&store, BOARD), sorted(vec![a, w]));
        assert_eq!(subjects_in(&store, INBOX), vec![u]);
    }
}
