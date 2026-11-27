//! M3 exit proof: oracle C1-C8 and partial-mode fixtures plan BYTE-IDENTICALLY
//! for a same-store fork and an equivalent mini-Quipu; a share-routed variant
//! fails that check; foreign-epoch, base-unavailable and base-mismatch refuse
//! writing nothing; R7 retry and the base advance.
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use super::super::local::{MaterializeSpec, MiniQuipu, materialize_on_legacy};
use super::super::tests::{FROM, Home, home};
use super::super::{MemberRole, MiniForkMember, mini_fork_entry};
use super::*;
use crate::schema::ROOT_GRAPH_IRI;
use crate::store::Datum;
use crate::store::forks_merge::ForkMerge;
use crate::types::{Op, Value};

const SHAPES: &str = "@prefix sh: <http://www.w3.org/ns/shacl#> .
<urn:one-shape> a sh:NodeShape ; sh:targetSubjectsOf <urn:one> ;
  sh:property [ sh:path <urn:one> ; sh:maxCount 1 ] .";
const T0: &str = "2026-01-02T00:00:00Z";
const TO: &str = "2026-02-01T00:00:00Z";
const TT: &str = "2026-03-01T00:00:00Z";
const TD: &str = "2026-06-01T00:00:00Z";
const ONE: &str = "urn:one";
const MANY: &str = "urn:many";

/// `(subject, predicate, value, valid_from, op)`
type W = (&'static str, &'static str, &'static str, &'static str, Op);

fn a(s: &'static str, p: &'static str, v: &'static str, at: &'static str) -> W {
    (s, p, v, at, Op::Assert)
}
fn r(s: &'static str, p: &'static str, v: &'static str) -> W {
    (s, p, v, T0, Op::Retract)
}

fn datums(store: &Store, ws: &[W]) -> Vec<Datum> {
    ws.iter()
        .map(|(s, p, v, at, op)| Datum {
            entity: store.intern(s).unwrap(),
            attribute: store.intern(p).unwrap(),
            value: Value::Str((*v).into()),
            valid_from: (*at).into(),
            valid_to: None,
            op: *op,
        })
        .collect()
}

fn write_home(home: &mut Store, g: i64, ws: &[W], at: &str) {
    if ws.is_empty() {
        return;
    }
    let d = datums(home, ws);
    home.ownership_operation(|s| s.transact_to_graph(&d, at, None, None, g))
        .unwrap();
}

struct Pair {
    home: Home,
    mini: MiniQuipu,
    path: PathBuf,
    fork: crate::store::forks::ForkInfo,
}

fn pair(dir: &Path, base: &[W], ours: &[W], theirs: &[W]) -> Pair {
    let mut home = home();
    home.legacy
        .ownership_operation(|s| s.load_shapes("home-shapes", SHAPES, FROM))
        .unwrap();
    write_home(&mut home.legacy, 0, base, T0);
    let cut = home.legacy.latest_tx_id().unwrap();
    let fork = home
        .legacy
        .ownership_operation(|s| s.fork_create("same", cut, T0, None))
        .unwrap();
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
    write_home(&mut home.legacy, fork.g, theirs, TT);
    if !theirs.is_empty() {
        let d = datums(mini.store(), theirs);
        mini.transact(ROOT_GRAPH_IRI, &d, TT, None).unwrap();
    }
    write_home(&mut home.legacy, 0, ours, TO);
    Pair {
        home,
        mini,
        path,
        fork,
    }
}

fn render(home: &Store, m: &ForkMerge) -> String {
    let term = |v: &Value| crate::rdf::value_to_term(home, v).unwrap().to_string();
    let mut out = String::new();
    for (step, ds) in [("receive", &m.receive), ("decide", &m.decide)] {
        for d in ds {
            writeln!(
                out,
                "{step} {:?} {} {} {} {} {:?}",
                d.op,
                term(&Value::Ref(d.entity)),
                term(&Value::Ref(d.attribute)),
                term(&d.value),
                d.valid_from,
                d.valid_to
            )
            .unwrap();
        }
    }
    for c in &m.conflicts {
        writeln!(out, "conflict {c:?}").unwrap();
    }
    writeln!(out, "held {}", m.held).unwrap();
    out
}

fn same_store_plan(p: &Pair, res: Option<&ForkResolutions>, mode: Mode, share: bool) -> String {
    let s = &p.home.legacy;
    let (base, ours, mut theirs) = s.fork_sides(&p.fork).unwrap();
    if share {
        // The share_merge input contract: incoming stamped at Td and no
        // close rows of its own (merge-behaviour.md C1/C3 "DIFF share").
        theirs.iter_mut().for_each(|f| f.valid_from = TD.into());
    }
    let g = p.fork.g;
    let closes = |e, a| {
        if share {
            Ok(BTreeMap::new())
        } else {
            s.fork_closes(g, e, a)
        }
    };
    render(
        s,
        &s.fork_merge(base, ours, theirs, res, &closes, mode, Some(TD))
            .unwrap(),
    )
}

fn mini_plan(p: &Pair, res: Option<&ForkResolutions>, mode: Mode) -> String {
    let entry = mini_fork_entry(&p.home.legacy, "agent-a").unwrap().unwrap();
    let theirs = open_theirs(&p.path, &entry).unwrap();
    let s = &p.home.legacy;
    let (_, m) = plan_member(
        s,
        &theirs,
        ROOT_GRAPH_IRI,
        entry.base.local_tx,
        res,
        mode,
        Some(TD),
    )
    .unwrap();
    render(s, &m)
}

struct Case {
    name: &'static str,
    base: Vec<W>,
    ours: Vec<W>,
    theirs: Vec<W>,
    decide: Option<(&'static str, &'static str, Vec<&'static str>)>,
    mode: Mode,
}

fn case(name: &'static str, base: Vec<W>, ours: Vec<W>, theirs: Vec<W>) -> Case {
    Case {
        name,
        base,
        ours,
        theirs,
        decide: None,
        mode: Mode::Atomic,
    }
}

fn oracle_cases() -> Vec<Case> {
    let x = "urn:x";
    let y = "urn:y";
    let ba = || vec![a(x, ONE, "a", T0)];
    let decided = |c: Case, v| Case {
        decide: Some((x, ONE, vec![v])),
        ..c
    };
    let mode = |c: Case, mode| Case { mode, ..c };
    let c4 = || {
        case(
            "C4",
            ba(),
            vec![r(x, ONE, "a"), a(x, ONE, "b", TO)],
            vec![r(x, ONE, "a"), a(x, ONE, "c", TT)],
        )
    };
    let partial = || {
        let mut c = c4();
        c.base.push(a(x, MANY, "m", T0));
        c.base.push(a(y, MANY, "n", T0));
        c.theirs.push(a(x, MANY, "m2", TT));
        c.theirs.push(a(y, MANY, "n2", TT));
        c
    };
    vec![
        case(
            "C1a",
            ba(),
            vec![],
            vec![r(x, ONE, "a"), a(x, ONE, "c", TT)],
        ),
        case(
            "C1b",
            ba(),
            vec![r(x, ONE, "a"), a(x, ONE, "b", TO)],
            vec![],
        ),
        case("C1c", ba(), vec![], vec![r(x, ONE, "a")]),
        case(
            "C2a",
            ba(),
            vec![r(x, ONE, "a"), a(x, ONE, "c", TO)],
            vec![r(x, ONE, "a"), a(x, ONE, "c", TT)],
        ),
        case("C2b", ba(), vec![r(x, ONE, "a")], vec![r(x, ONE, "a")]),
        case(
            "C3a",
            vec![a(x, MANY, "a", T0)],
            vec![a(x, MANY, "b", TO)],
            vec![a(x, MANY, "c", TT)],
        ),
        case(
            "C3c",
            vec![a(x, MANY, "a", T0)],
            vec![a(x, MANY, "b", TO)],
            vec![r(x, MANY, "a")],
        ),
        c4(),
        decided(c4(), "c"),
        decided(c4(), "b"),
        decided(c4(), "a"),
        decided(c4(), "d"),
        case(
            "C5a",
            ba(),
            vec![r(x, ONE, "a")],
            vec![r(x, ONE, "a"), a(x, ONE, "c", TT)],
        ),
        case(
            "C5b",
            ba(),
            vec![r(x, ONE, "a"), a(x, ONE, "c", TO)],
            vec![r(x, ONE, "a")],
        ),
        case(
            "C5c",
            vec![a(x, MANY, "a", T0)],
            vec![r(x, MANY, "a")],
            vec![a(x, MANY, "c", TT)],
        ),
        case(
            "C7",
            ba(),
            vec![r(x, ONE, "a"), a(x, ONE, "a", TO)],
            vec![r(x, ONE, "a"), a(x, ONE, "c", TT)],
        ),
        case("C8", ba(), vec![], vec![]),
        mode(partial(), Mode::PartialSlot),
        mode(partial(), Mode::PartialEntity),
        partial(),
    ]
}

fn resolutions(p: &Pair, c: &Case) -> Option<ForkResolutions> {
    c.decide.as_ref().map(|(s, pr, vs)| {
        let st = &p.home.legacy;
        let key = (
            st.lookup(s).unwrap().unwrap(),
            st.lookup(pr).unwrap().unwrap(),
        );
        let vals = vs.iter().map(|v| Value::Str((*v).into())).collect();
        ForkResolutions::from([(key, vals)])
    })
}

#[test]
fn oracle_cases_plan_byte_identically_for_a_same_store_fork_and_a_mini_quipu() {
    for c in oracle_cases() {
        let dir = tempfile::tempdir().unwrap();
        let p = pair(dir.path(), &c.base, &c.ours, &c.theirs);
        let res = resolutions(&p, &c);
        let same = same_store_plan(&p, res.as_ref(), c.mode, false);
        let mini = mini_plan(&p, res.as_ref(), c.mode);
        assert_eq!(same, mini, "{} ({:?}) differs", c.name, c.mode);
        if std::env::var_os("MINI_MERGE_SHOW").is_some() {
            eprintln!("== {} {:?}\n{same}", c.name, c.mode);
        }
        let expect = match (c.name, c.decide.as_ref().map(|d| d.2[0])) {
            ("C1a", _) => &["receive Assert", TT][..],
            ("C1c", _) => &["receive Retract"][..],
            ("C1b" | "C8" | "C2a" | "C2b", _) => &["held 0"][..],
            ("C4", None) if c.mode == Mode::Atomic => &["conflict DecisionRecord"][..],
            ("C4", Some("c")) => &["receive Assert", "decide Retract", "decide Assert"][..],
            ("C4", Some("b")) => &["receive Assert"][..],
            ("C4", None) if c.mode == Mode::PartialSlot => &["held 1"][..],
            ("C4", None) => &["held 2"][..],
            ("C3a" | "C5c", _) => &["receive Assert"][..],
            _ => &[][..],
        };
        for e in expect {
            assert!(
                same.contains(e),
                "{} ({:?}) lacks {e:?}:\n{same}",
                c.name,
                c.mode
            );
        }
    }
}

#[test]
fn partial_modes_hold_the_conflict_and_apply_the_rest() {
    let cases = oracle_cases();
    let held = |m: Mode| {
        let c = cases
            .iter()
            .find(|c| c.mode == m && c.theirs.len() > 2)
            .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let p = pair(dir.path(), &c.base, &c.ours, &c.theirs);
        mini_plan(&p, None, m)
    };
    let slot = held(Mode::PartialSlot);
    let entity = held(Mode::PartialEntity);
    assert!(slot.contains("\"m2\"") && slot.contains("\"n2\""), "{slot}");
    assert!(
        !entity.contains("\"m2\"") && entity.contains("\"n2\""),
        "{entity}"
    );
    assert!(
        !slot.contains("\"c\" "),
        "a held slot is not received: {slot}"
    );
}

#[test]
fn sabotage_share_merge_inputs_fail_the_identical_plan_check() {
    let c = oracle_cases()
        .into_iter()
        .find(|c| c.name == "C1a")
        .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let p = pair(dir.path(), &c.base, &c.ours, &c.theirs);
    let share = same_store_plan(&p, None, Mode::Atomic, true);
    assert_ne!(share, mini_plan(&p, None, Mode::Atomic));
    assert_eq!(
        same_store_plan(&p, None, Mode::Atomic, false),
        mini_plan(&p, None, Mode::Atomic)
    );
}

fn footprint(store: &Store) -> (i64, i64, String) {
    let q = |sql: &str| store.conn.query_row(sql, [], |r| r.get(0)).unwrap();
    let entry: String = store
        .conn
        .query_row(
            "SELECT entry FROM mini_quipu_forks WHERE name='agent-a'",
            [],
            |r| r.get(0),
        )
        .unwrap_or_default();
    (
        q("SELECT count(*) FROM facts"),
        q("SELECT count(*) FROM transactions"),
        entry,
    )
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

fn c1a(dir: &Path) -> Pair {
    pair(
        dir,
        &[a("urn:x", ONE, "a", T0)],
        &[],
        &[r("urn:x", ONE, "a"), a("urn:x", ONE, "c", TT)],
    )
}

fn assert_refuses(p: &mut Pair, path: &Path, code: &str) {
    let (home0, mini0) = (footprint(&p.home.legacy), footprint(p.mini.store()));
    let err = merge_on_legacy(&p.home.catalog, &mut p.home.legacy, path, &request()).unwrap_err();
    assert!(err.to_string().contains(code), "{err}");
    assert_eq!(footprint(&p.home.legacy), home0, "{code} wrote at home");
    assert_eq!(footprint(p.mini.store()), mini0, "{code} wrote locally");
}

fn tamper(p: &Pair, edit: impl FnOnce(&mut MiniForkEntry)) {
    let mut e = mini_fork_entry(&p.home.legacy, "agent-a").unwrap().unwrap();
    edit(&mut e);
    p.home
        .legacy
        .conn
        .execute(
            "UPDATE mini_quipu_forks SET entry=?1 WHERE name='agent-a'",
            [serde_json::to_string(&e).unwrap()],
        )
        .unwrap();
}

#[test]
fn foreign_epoch_refuses_with_nothing_written() {
    let dir = tempfile::tempdir().unwrap();
    let mut p = c1a(dir.path());
    let copy = dir.path().join("home-copy.db");
    p.home
        .legacy
        .conn
        .execute("VACUUM INTO ?1", [copy.to_str().unwrap()])
        .unwrap();
    assert_refuses(&mut p, &copy, "foreign-epoch");
    tamper(&p, |e| e.log_epoch = "another-epoch".into());
    let path = p.path.clone();
    assert_refuses(&mut p, &path, "foreign-epoch");
}

#[test]
fn base_unavailable_refuses_with_nothing_written() {
    let dir = tempfile::tempdir().unwrap();
    let mut p = c1a(dir.path());
    tamper(&p, |e| e.base.local_tx = 1_000_000);
    let path = p.path.clone();
    assert_refuses(&mut p, &path, "base-unavailable");
}

#[test]
fn base_mismatch_refuses_with_nothing_written() {
    let dir = tempfile::tempdir().unwrap();
    let mut p = c1a(dir.path());
    tamper(&p, |e| e.base_digest = "0".repeat(64));
    let path = p.path.clone();
    assert_refuses(&mut p, &path, "base-mismatch");
}

#[test]
fn atomic_conflict_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let x = "urn:x";
    let mut p = pair(
        dir.path(),
        &[a(x, ONE, "a", T0)],
        &[r(x, ONE, "a"), a(x, ONE, "b", TO)],
        &[r(x, ONE, "a"), a(x, ONE, "c", TT)],
    );
    let before = footprint(&p.home.legacy);
    let out = merge_on_legacy(
        &p.home.catalog,
        &mut p.home.legacy,
        &p.path.clone(),
        &request(),
    )
    .unwrap();
    assert!(
        matches!(out, MergeBack::Conflicts(ref c) if c.len() == 1),
        "{out:?}"
    );
    assert_eq!(footprint(&p.home.legacy), before);
}

#[test]
fn r7_retry_reports_already_applied_and_the_base_advances() {
    let dir = tempfile::tempdir().unwrap();
    let mut p = c1a(dir.path());
    let path = p.path.clone();
    let MergeBack::Merged(first) =
        merge_on_legacy(&p.home.catalog, &mut p.home.legacy, &path, &request()).unwrap()
    else {
        panic!("expected a merge");
    };
    assert!(
        first.receive_tx[0] > 0 && first.decide_tx == vec![0],
        "{first:?}"
    );
    let entry = mini_fork_entry(&p.home.legacy, "agent-a").unwrap().unwrap();
    assert_eq!(entry.base.local_tx, first.receive_tx[0]);
    assert_eq!(
        entry.merged_head,
        Some(p.mini.store().latest_tx_id().unwrap())
    );
    let journal = member_journal(&p.home.legacy, entry.base.local_tx, ROOT_GRAPH_IRI).unwrap();
    assert_eq!(entry.base_digest, base_digest(&[journal]).unwrap());
    let src: String = p
        .home
        .legacy
        .conn
        .query_row(
            "SELECT source FROM transactions WHERE id=?1",
            [first.receive_tx[0]],
            |r| r.get(0),
        )
        .unwrap();
    assert!(src.contains(&format!("merge={}", first.merge_id)), "{src}");

    let before = footprint(&p.home.legacy);
    let again = merge_on_legacy(&p.home.catalog, &mut p.home.legacy, &path, &request()).unwrap();
    assert!(
        matches!(again, MergeBack::AlreadyApplied(ref r) if *r == first),
        "{again:?}"
    );
    assert_eq!(footprint(&p.home.legacy), before, "a retry wrote");
}

fn merged(p: &mut Pair, req: &MergeRequest<'_>) -> MergeReceipt {
    let path = p.path.clone();
    match merge_on_legacy(&p.home.catalog, &mut p.home.legacy, &path, req).unwrap() {
        MergeBack::Merged(r) => r,
        other => panic!("expected a merge, got {other:?}"),
    }
}

/// Review (#1318, eph-review-7f44b0) 1: once a merge wrote at home the base
/// sits at the home head, so a later merge would land the mini's stale values
/// as one-sided changes (reverting home-only work, dropping held slots).
#[test]
fn a_second_merge_after_a_written_one_refuses_naming_the_earlier_merge() {
    let dir = tempfile::tempdir().unwrap();
    let mut p = c1a(dir.path());
    let first = merged(&mut p, &request());
    // Home changes a slot the mini never touched, then the mini writes again.
    write_home(&mut p.home.legacy, 0, &[a("urn:y", ONE, "home", TO)], TO);
    let d = datums(p.mini.store(), &[a("urn:z", ONE, "mini", TT)]);
    p.mini.transact(ROOT_GRAPH_IRI, &d, TT, None).unwrap();
    let (home0, mini0) = (footprint(&p.home.legacy), footprint(p.mini.store()));
    let path = p.path.clone();
    let err = merge_on_legacy(&p.home.catalog, &mut p.home.legacy, &path, &request()).unwrap_err();
    let text = err.to_string();
    assert!(
        text.contains("second-merge") && text.contains(&first.merge_id),
        "{text}"
    );
    assert_eq!(footprint(&p.home.legacy), home0);
    assert_eq!(footprint(p.mini.store()), mini0);
}

/// Review 2: the mode is part of the R7 key and of a partial merge's id.
#[test]
fn the_same_input_under_another_mode_is_not_already_applied() {
    let dir = tempfile::tempdir().unwrap();
    let mut p = c1a(dir.path());
    let entity = MergeRequest {
        mode: MergeMode::PartialEntity,
        ..request()
    };
    let first = merged(&mut p, &entity);
    let path = p.path.clone();
    let again = merge_on_legacy(&p.home.catalog, &mut p.home.legacy, &path, &entity).unwrap();
    assert!(
        matches!(again, MergeBack::AlreadyApplied(ref r) if *r == first),
        "{again:?}"
    );
    let slot = MergeRequest {
        mode: MergeMode::PartialSlot,
        ..request()
    };
    let other = merge_on_legacy(&p.home.catalog, &mut p.home.legacy, &path, &slot);
    assert!(
        !matches!(other, Ok(MergeBack::AlreadyApplied(_))),
        "another mode reused the old receipt: {other:?}"
    );
    // A partial mode's id differs from the atomic one for the same input.
    let dir = tempfile::tempdir().unwrap();
    let mut atomic = c1a(dir.path());
    assert_ne!(merged(&mut atomic, &request()).merge_id, first.merge_id);
}

/// Review 3: the R7 key carries a digest of the resolutions themselves, so
/// different resolutions under one caller hash never share a key. (An
/// end-to-end decided merge cannot be built here yet: deciding a conflict
/// closes a value, and advancing the base over that bounded interval refuses,
/// a pre-existing limit reported on #1318.)
#[test]
fn different_resolutions_never_share_an_r7_key() {
    let key = (1, 2);
    let one = ForkResolutions::from([(key, vec![Value::Str("a".into())])]);
    let two = ForkResolutions::from([(key, vec![Value::Str("z".into())])]);
    let both = ForkResolutions::from([
        (key, vec![Value::Str("a".into()), Value::Str("b".into())]),
        ((3, 4), vec![Value::Str("c".into())]),
    ]);
    let swapped = ForkResolutions::from([
        ((3, 4), vec![Value::Str("c".into())]),
        (key, vec![Value::Str("b".into()), Value::Str("a".into())]),
    ]);
    assert_ne!(
        super::resolved_digest(Some(&one)),
        super::resolved_digest(Some(&two))
    );
    assert_ne!(
        super::resolved_digest(Some(&one)),
        super::resolved_digest(None)
    );
    assert_eq!(
        super::resolved_digest(Some(&both)),
        super::resolved_digest(Some(&swapped))
    );
}

/// Review c0dd94 1 (sattler ruling: ship fail-closed): a decided conflict
/// merge closes a value, the base cannot advance over that bounded interval
/// yet, so the whole merge refuses by name and writes nothing.
#[test]
fn a_decided_conflict_merge_refuses_by_name_with_nothing_written() {
    let dir = tempfile::tempdir().unwrap();
    let x = "urn:x";
    let mut p = pair(
        dir.path(),
        &[a(x, ONE, "a", T0)],
        &[r(x, ONE, "a"), a(x, ONE, "b", TO)],
        &[r(x, ONE, "a"), a(x, ONE, "c", TT)],
    );
    let key = {
        let st = &p.home.legacy;
        (
            st.lookup(x).unwrap().unwrap(),
            st.lookup(ONE).unwrap().unwrap(),
        )
    };
    let theirs = ForkResolutions::from([(key, vec![Value::Str("c".into())])]);
    let req = MergeRequest {
        decided: Some((&theirs, "decisions", "reviewer")),
        ..request()
    };
    let (home0, mini0) = (footprint(&p.home.legacy), footprint(p.mini.store()));
    let path = p.path.clone();
    let err = merge_on_legacy(&p.home.catalog, &mut p.home.legacy, &path, &req).unwrap_err();
    assert!(err.to_string().contains("decided-bounded-base"), "{err}");
    assert!(err.to_string().contains("nothing was written"), "{err}");
    assert_eq!(
        footprint(&p.home.legacy),
        home0,
        "a refused decided merge wrote at home"
    );
    assert_eq!(footprint(p.mini.store()), mini0);
}
