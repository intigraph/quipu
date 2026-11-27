//! aegis-abt slice after M3: decided merges and later merges. A later merge
//! plans against the mini-Quipu's own state at the merged head (the last
//! common point), so slots the mini did not touch since then keep home:
//! home-only changes, held slots and decisions are never overridden by the
//! mini's unchanged values. A remainder of a partial merge is its own merge
//! naming the earlier one; a new incoming refuses while a remainder is held.
use super::super::super::tests::LEG;
use super::*;

fn values(store: &Store, s: &str, p: &str) -> Vec<String> {
    let (Some(e), Some(at)) = (store.lookup(s).unwrap(), store.lookup(p).unwrap()) else {
        return Vec::new();
    };
    let mut out: Vec<String> = store
        .current_facts_in_graph(0)
        .unwrap()
        .into_iter()
        .filter(|f| f.entity == e && f.attribute == at)
        .map(|f| match f.value {
            Value::Str(v) => v,
            other => format!("{other:?}"),
        })
        .collect();
    out.sort();
    out
}

fn mini_write(p: &mut Pair, ws: &[W]) {
    let d = datums(p.mini.store(), ws);
    p.mini.transact(ROOT_GRAPH_IRI, &d, TT, None).unwrap();
}

fn key(p: &Pair, s: &str, at: &str) -> (i64, i64) {
    let st = &p.home.legacy;
    (
        st.lookup(s).unwrap().unwrap(),
        st.lookup(at).unwrap().unwrap(),
    )
}

/// x conflicts (ours b, theirs c); y is a one-sided mini change.
fn conflict_pair(dir: &Path) -> Pair {
    let (x, y) = ("urn:x", "urn:y");
    pair(
        dir,
        &[a(x, ONE, "a", T0), a(y, ONE, "y0", T0)],
        &[r(x, ONE, "a"), a(x, ONE, "b", TO)],
        &[
            r(x, ONE, "a"),
            a(x, ONE, "c", TT),
            r(y, ONE, "y0"),
            a(y, ONE, "y1", TT),
        ],
    )
}

fn decided<'a>(res: &'a ForkResolutions) -> MergeRequest<'a> {
    MergeRequest {
        decided: Some((res, "decisions", "reviewer")),
        ..request()
    }
}

/// R1: a decided conflict merge writes, and its retry is already-applied
/// with the original transaction ids.
#[test]
fn later_r1_a_decided_conflict_merge_writes_and_retries_as_already_applied() {
    let dir = tempfile::tempdir().unwrap();
    let mut p = conflict_pair(dir.path());
    let res = ForkResolutions::from([(key(&p, "urn:x", ONE), vec![Value::Str("c".into())])]);
    let first = merged(&mut p, &decided(&res));
    assert_eq!(values(&p.home.legacy, "urn:x", ONE), ["c"]);
    assert_eq!(values(&p.home.legacy, "urn:y", ONE), ["y1"]);
    let before = footprint(&p.home.legacy);
    let path = p.path.clone();
    let again =
        merge_on_legacy(&p.home.catalog, &mut p.home.legacy, &path, &decided(&res)).unwrap();
    assert!(
        matches!(again, MergeBack::AlreadyApplied(ref r) if *r == first),
        "{again:?}"
    );
    assert_eq!(footprint(&p.home.legacy), before, "a retry wrote");
}

/// R2: a home-only change made after the fork survives a later merge.
#[test]
fn later_r2_a_second_merge_keeps_a_home_only_change() {
    let dir = tempfile::tempdir().unwrap();
    let (x, y) = ("urn:x", "urn:y");
    let mut p = pair(
        dir.path(),
        &[a(x, ONE, "a", T0), a(y, ONE, "y0", T0)],
        &[r(y, ONE, "y0"), a(y, ONE, "home", TO)],
        &[r(x, ONE, "a"), a(x, ONE, "c", TT)],
    );
    merged(&mut p, &request());
    mini_write(&mut p, &[a("urn:z", ONE, "mini", TT)]);
    merged(&mut p, &request());
    assert_eq!(
        values(&p.home.legacy, y, ONE),
        ["home"],
        "home-only reverted"
    );
    assert_eq!(values(&p.home.legacy, "urn:z", ONE), ["mini"]);
    assert_eq!(values(&p.home.legacy, x, ONE), ["c"]);
}

/// R3: a decision survives a later merge that left its slot unchanged.
#[test]
fn later_r3_a_second_merge_keeps_a_decision_the_mini_did_not_revisit() {
    let dir = tempfile::tempdir().unwrap();
    let mut p = conflict_pair(dir.path());
    let res = ForkResolutions::from([(key(&p, "urn:x", ONE), vec![Value::Str("b".into())])]);
    merged(&mut p, &decided(&res));
    mini_write(&mut p, &[a("urn:z", ONE, "mini", TT)]);
    merged(&mut p, &request());
    assert_eq!(
        values(&p.home.legacy, "urn:x", ONE),
        ["b"],
        "decision overridden"
    );
    assert_eq!(values(&p.home.legacy, "urn:z", ONE), ["mini"]);
}

/// R4: the mini revisiting a decided slot conflicts again; it never lands
/// silently as a one-sided change.
#[test]
fn later_r4_a_mini_change_on_a_decided_slot_conflicts() {
    let dir = tempfile::tempdir().unwrap();
    let mut p = conflict_pair(dir.path());
    let res = ForkResolutions::from([(key(&p, "urn:x", ONE), vec![Value::Str("b".into())])]);
    merged(&mut p, &decided(&res));
    mini_write(&mut p, &[r("urn:x", ONE, "c"), a("urn:x", ONE, "d", TT)]);
    let before = footprint(&p.home.legacy);
    let path = p.path.clone();
    let out = merge_on_legacy(&p.home.catalog, &mut p.home.legacy, &path, &request()).unwrap();
    assert!(
        matches!(out, MergeBack::Conflicts(ref c) if c.len() == 1),
        "{out:?}"
    );
    assert_eq!(footprint(&p.home.legacy), before);
    assert_eq!(values(&p.home.legacy, "urn:x", ONE), ["b"]);
}

fn partial() -> MergeRequest<'static> {
    MergeRequest {
        mode: MergeMode::PartialSlot,
        ..request()
    }
}

/// R5: a new incoming refuses while an earlier merge still holds a slot.
#[test]
fn later_r5_a_new_incoming_with_a_held_remainder_refuses() {
    let dir = tempfile::tempdir().unwrap();
    let mut p = conflict_pair(dir.path());
    let first = merged(&mut p, &partial());
    assert_eq!(first.held, 1, "{first:?}");
    mini_write(&mut p, &[a("urn:z", ONE, "mini", TT)]);
    let (home0, mini0) = (footprint(&p.home.legacy), footprint(p.mini.store()));
    let path = p.path.clone();
    let err = merge_on_legacy(&p.home.catalog, &mut p.home.legacy, &path, &request()).unwrap_err();
    let text = err.to_string();
    assert!(
        text.contains("remainder-pending") && text.contains(&first.merge_id),
        "{text}"
    );
    assert_eq!(footprint(&p.home.legacy), home0);
    assert_eq!(footprint(p.mini.store()), mini0);
}

/// R6: the remainder of a partial merge is its own merge naming the earlier
/// one; it writes only the held slot and its retry is a no-op.
#[test]
fn later_r6_a_remainder_names_the_earlier_merge_and_writes_only_the_held_slot() {
    let dir = tempfile::tempdir().unwrap();
    let mut p = conflict_pair(dir.path());
    let first = merged(&mut p, &partial());
    assert_eq!(values(&p.home.legacy, "urn:y", ONE), ["y1"]);
    assert_eq!(
        values(&p.home.legacy, "urn:x", ONE),
        ["b"],
        "held slot moved"
    );
    let res = ForkResolutions::from([(key(&p, "urn:x", ONE), vec![Value::Str("c".into())])]);
    let rest = merged(&mut p, &decided(&res));
    assert_ne!(rest.merge_id, first.merge_id);
    assert_eq!(values(&p.home.legacy, "urn:x", ONE), ["c"]);
    assert_eq!(values(&p.home.legacy, "urn:y", ONE), ["y1"]);
    let tx = rest
        .decide_tx
        .iter()
        .chain(&rest.receive_tx)
        .copied()
        .max()
        .unwrap();
    let src: String = p
        .home
        .legacy
        .conn
        .query_row("SELECT source FROM transactions WHERE id=?1", [tx], |r| {
            r.get(0)
        })
        .unwrap();
    assert!(
        src.contains(&first.merge_id),
        "remainder does not name the earlier merge: {src}"
    );
    let y = key(&p, "urn:y", ONE);
    let rewritten: i64 = p
        .home
        .legacy
        .conn
        .query_row(
            "SELECT count(*) FROM facts WHERE e=?1 AND a=?2 AND tx>?3",
            params![y.0, y.1, first.receive_tx[0]],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(rewritten, 0, "the remainder rewrote a received slot");
    let before = footprint(&p.home.legacy);
    let path = p.path.clone();
    let again =
        merge_on_legacy(&p.home.catalog, &mut p.home.legacy, &path, &decided(&res)).unwrap();
    assert!(
        matches!(again, MergeBack::AlreadyApplied(ref r) if *r == rest),
        "{again:?}"
    );
    assert_eq!(footprint(&p.home.legacy), before);
}

/// R7: a later merge refuses if the mini's history up to the merged head
/// was rewritten, writing nothing.
#[test]
fn later_r7_a_rewritten_mini_history_refuses_base_mismatch() {
    let dir = tempfile::tempdir().unwrap();
    let mut p = c1a(dir.path());
    merged(&mut p, &request());
    mini_write(&mut p, &[a("urn:z", ONE, "mini", TT)]);
    let forged = {
        let st = p.mini.store();
        st.conn
            .execute(
                "UPDATE facts SET v=?1 WHERE v=?2",
                params![
                    Value::Str("forged".into()).to_bytes(),
                    Value::Str("c".into()).to_bytes()
                ],
            )
            .unwrap()
    };
    assert!(forged > 0, "the forgery touched no row");
    let path = p.path.clone();
    assert_refuses(&mut p, &path, "base-mismatch");
}

/// R8 (review 1): an entry M3 advanced (merged head, no last merge) has a
/// home cut that already folds in home-only changes; a later merge refuses
/// `second-merge` instead of planning on it, writing nothing.
#[test]
fn later_r8_an_m3_era_entry_refuses_second_merge() {
    let dir = tempfile::tempdir().unwrap();
    let mut p = c1a(dir.path());
    merged(&mut p, &request());
    tamper(&p, |e| e.last_merge = None);
    mini_write(&mut p, &[a("urn:z", ONE, "mini", TT)]);
    let path = p.path.clone();
    assert_refuses(&mut p, &path, "second-merge");
}

/// A decided merge, then a partial merge of a new head that holds the
/// revisited slot: its remainder plans on a mini base.
fn held_on_a_mini_base(dir: &Path) -> (Pair, MergeReceipt) {
    let mut p = conflict_pair(dir);
    let res = ForkResolutions::from([(key(&p, "urn:x", ONE), vec![Value::Str("b".into())])]);
    merged(&mut p, &decided(&res));
    mini_write(
        &mut p,
        &[
            r("urn:x", ONE, "c"),
            a("urn:x", ONE, "d", TT),
            r("urn:y", ONE, "y1"),
            a("urn:y", ONE, "y2", TT),
        ],
    );
    let held = merged(&mut p, &partial());
    assert_eq!(held.held, 1, "{held:?}");
    let entry = mini_fork_entry(&p.home.legacy, "agent-a").unwrap().unwrap();
    assert!(
        matches!(
            entry.last_merge.as_ref().unwrap().base,
            MergeBase::Mini { .. }
        ),
        "{entry:?}"
    );
    (p, held)
}

/// R9 (review 2): a remainder re-verifies its mini base. A history rewritten
/// between the partial merge and its remainder refuses `base-mismatch`.
#[test]
fn later_r9_a_remainder_on_a_rewritten_mini_base_refuses_base_mismatch() {
    let dir = tempfile::tempdir().unwrap();
    let (mut p, _) = held_on_a_mini_base(dir.path());
    let forged = p
        .mini
        .store()
        .conn
        .execute(
            "UPDATE facts SET v=?1 WHERE v=?2",
            params![
                Value::Str("forged".into()).to_bytes(),
                Value::Str("y1".into()).to_bytes()
            ],
        )
        .unwrap();
    assert!(forged > 0, "the forgery touched no row");
    let res = ForkResolutions::from([(key(&p, "urn:x", ONE), vec![Value::Str("d".into())])]);
    let (home0, mini0) = (footprint(&p.home.legacy), footprint(p.mini.store()));
    let path = p.path.clone();
    let err =
        merge_on_legacy(&p.home.catalog, &mut p.home.legacy, &path, &decided(&res)).unwrap_err();
    assert!(err.to_string().contains("base-mismatch"), "{err}");
    assert_eq!(
        footprint(&p.home.legacy),
        home0,
        "base-mismatch wrote at home"
    );
    assert_eq!(
        footprint(p.mini.store()),
        mini0,
        "base-mismatch wrote locally"
    );
}

/// R10 (review 3): a later merge whose mini state at the merged head cannot
/// be read refuses `base-unavailable`, writing nothing.
#[test]
fn later_r10_an_unreadable_mini_base_refuses_base_unavailable() {
    let dir = tempfile::tempdir().unwrap();
    let mut p = c1a(dir.path());
    merged(&mut p, &request());
    mini_write(&mut p, &[a("urn:z", ONE, "mini", TT)]);
    let broken = p
        .mini
        .store()
        .conn
        .execute(
            "UPDATE facts SET v=X'FFFF' WHERE v=?1",
            params![Value::Str("c".into()).to_bytes()],
        )
        .unwrap();
    assert!(broken > 0, "the corruption touched no row");
    let path = p.path.clone();
    assert_refuses(&mut p, &path, "base-unavailable");
}

/// Two writable members: ROOT and the legacy set's other graph.
fn multi_pair(dir: &Path) -> (Pair, i64, i64) {
    let mut p = pair(dir, &[a("urn:x", ONE, "a", T0)], &[], &[]);
    // Re-materialize with both members of the legacy set writable.
    let path = dir.join("multi.db");
    let spec = MaterializeSpec {
        name: "agent-m".into(),
        members: vec![
            MiniForkMember {
                iri: ROOT_GRAPH_IRI.into(),
                role: MemberRole::Writable,
            },
            MiniForkMember {
                iri: LEG.into(),
                role: MemberRole::Writable,
            },
        ],
        created_at: FROM.into(),
    };
    p.mini = materialize_on_legacy(&p.home.catalog, &mut p.home.legacy, &spec, &path).unwrap();
    p.path = path;
    let home_leg = p.home.legacy.committed_graph_id(LEG).unwrap().unwrap();
    let mini_leg = p.mini.store().committed_graph_id(LEG).unwrap().unwrap();
    (p, home_leg, mini_leg)
}

fn multi_request() -> MergeRequest<'static> {
    MergeRequest {
        name: "agent-m",
        ..request()
    }
}

fn leg_write(p: &mut Pair, ws: &[W]) {
    let d = datums(p.mini.store(), ws);
    p.mini.transact(LEG, &d, TT, None).unwrap();
}

fn leg_values(store: &Store, g: i64, s: &str) -> Vec<String> {
    let (Some(e), Some(at)) = (store.lookup(s).unwrap(), store.lookup(ONE).unwrap()) else {
        return Vec::new();
    };
    let mut out: Vec<String> = store
        .current_facts_in_graph(g)
        .unwrap()
        .into_iter()
        .filter(|f| f.entity == e && f.attribute == at)
        .map(|f| match f.value {
            Value::Str(v) => v,
            other => format!("{other:?}"),
        })
        .collect();
    out.sort();
    out
}

/// R11 (review #3): a later merge over two writable members keeps a
/// home-only change in the non-ROOT member and applies the mini's new value.
#[test]
fn later_r11_a_multi_member_later_merge_keeps_home_only_changes_in_each_member() {
    let dir = tempfile::tempdir().unwrap();
    let (mut p, home_leg, _) = multi_pair(dir.path());
    leg_write(&mut p, &[a("urn:n", ONE, "first", TT)]);
    mini_write(&mut p, &[r("urn:x", ONE, "a"), a("urn:x", ONE, "c", TT)]);
    merged(&mut p, &multi_request());
    assert_eq!(leg_values(&p.home.legacy, home_leg, "urn:n"), ["first"]);
    // Home-only change in LEG after merge 1; a new mini value in LEG.
    write_home(
        &mut p.home.legacy,
        home_leg,
        &[a("urn:h", ONE, "home", TO)],
        TD,
    );
    leg_write(&mut p, &[a("urn:m", ONE, "second", TT)]);
    let later = merged(&mut p, &multi_request());
    assert_eq!(
        later.receive_tx.len(),
        2,
        "one receive per member: {later:?}"
    );
    assert_eq!(
        leg_values(&p.home.legacy, home_leg, "urn:h"),
        ["home"],
        "home-only reverted"
    );
    assert_eq!(leg_values(&p.home.legacy, home_leg, "urn:m"), ["second"]);
    assert_eq!(leg_values(&p.home.legacy, home_leg, "urn:n"), ["first"]);
    assert_eq!(values(&p.home.legacy, "urn:x", ONE), ["c"]);
}

/// R12 (review 3): a remainder that decides every held slot to ours is
/// recorded as the last merge, so its held count is current and a new head
/// merges instead of refusing `remainder-pending`. (Deciding ours still
/// receives theirs as a closed interval here; `advance` also records a
/// remainder that writes no transaction at all.)
#[test]
fn later_r12_a_remainder_decided_to_ours_clears_the_hold() {
    let dir = tempfile::tempdir().unwrap();
    let mut p = conflict_pair(dir.path());
    let first = merged(&mut p, &partial());
    assert_eq!(first.held, 1, "{first:?}");
    let res = ForkResolutions::from([(key(&p, "urn:x", ONE), vec![Value::Str("b".into())])]);
    let rest = merged(&mut p, &decided(&res));
    assert_ne!(rest.merge_id, first.merge_id);
    assert_eq!(rest.held, 0, "{rest:?}");
    let last = mini_fork_entry(&p.home.legacy, "agent-a")
        .unwrap()
        .unwrap()
        .last_merge
        .unwrap();
    assert_eq!(
        (last.merge_id.as_str(), last.held),
        (rest.merge_id.as_str(), 0)
    );
    mini_write(&mut p, &[a("urn:z", ONE, "mini", TT)]);
    let next = merged(&mut p, &request());
    assert_ne!(next.merge_id, rest.merge_id);
    assert_eq!(values(&p.home.legacy, "urn:z", ONE), ["mini"]);
    assert_eq!(values(&p.home.legacy, "urn:x", ONE), ["b"], "decision lost");
    assert_eq!(values(&p.home.legacy, "urn:y", ONE), ["y1"]);
}

/// R13 (review eph-review-6131b4 #1, the no-transaction path): a remainder that writes no
/// transaction still completes the earlier merge. Here home converges on the
/// mini's value after the partial merge, so the remainder's plan is empty.
/// The entry must record that it holds nothing any more: its retry is
/// already-applied, and a new head merges instead of refusing
/// `remainder-pending` forever.
#[test]
fn later_r13_a_remainder_that_writes_nothing_releases_the_hold() {
    let dir = tempfile::tempdir().unwrap();
    let mut p = conflict_pair(dir.path());
    let first = merged(&mut p, &partial());
    assert_eq!(first.held, 1, "{first:?}");
    write_home(
        &mut p.home.legacy,
        0,
        &[r("urn:x", ONE, "b"), a("urn:x", ONE, "c", TT)],
        TD,
    );
    let rest = merged(&mut p, &request());
    assert_eq!(rest.held, 0, "{rest:?}");
    assert!(
        rest.receive_tx
            .iter()
            .chain(&rest.decide_tx)
            .all(|tx| *tx == 0),
        "the fixture must exercise the no-transaction remainder: {rest:?}"
    );
    let entry = mini_fork_entry(&p.home.legacy, "agent-a").unwrap().unwrap();
    let last = entry.last_merge.expect("a merge was recorded");
    assert_eq!(
        (last.merge_id.as_str(), last.held),
        (rest.merge_id.as_str(), 0)
    );
    let path = p.path.clone();
    let again = merge_on_legacy(&p.home.catalog, &mut p.home.legacy, &path, &request()).unwrap();
    assert!(
        matches!(again, MergeBack::AlreadyApplied(ref r) if *r == rest),
        "{again:?}"
    );
    mini_write(&mut p, &[a("urn:z", ONE, "mini", TT)]);
    merged(&mut p, &request());
    assert_eq!(values(&p.home.legacy, "urn:z", ONE), ["mini"]);
    assert_eq!(values(&p.home.legacy, "urn:x", ONE), ["c"]);
}
