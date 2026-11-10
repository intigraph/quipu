use super::*;
use ledger::{Disposition, decide, verify};
use rusqlite::Connection;

fn graph(text: &str) -> HashSet<Triple> {
    crate::share_merge::parse_graph(text, "crosswalk fixture").unwrap()
}
fn context() -> Context {
    Context {
        operation: "fork".into(),
        installation: "local-store-a".into(),
        base: "base-snapshot-a".into(),
        ours: "local-snapshot-a".into(),
        theirs: "incoming-snapshot-a".into(),
        scope: "urn:example:scope".into(),
        profile: "profile-a".into(),
    }
}
fn set(extra: &str) -> ExternalSet {
    ExternalSet::parse(&format!("#mapping_set_id: urn:example:set\nsubject_id\tpredicate_id\tobject_id\tmapping_justification\tauthor_id\taccepted\nurn:incoming:a\thttp://www.w3.org/2004/02/skos/core#exactMatch\turn:local:a\thttps://w3id.org/semapv/vocab/LexicalMatching\turn:attacker\ttrue\n{extra}")).unwrap()
}
fn sides() -> (HashSet<Triple>, HashSet<Triple>) {
    (
        graph("<urn:local:a> <urn:example:label> \"a\" ."),
        graph("<urn:incoming:a> <urn:example:label> \"a\" ."),
    )
}
fn plan(set: &ExternalSet, context: Context) -> Plan {
    let (ours, theirs) = sides();
    Plan::prepare(
        set,
        context,
        &ours,
        &theirs,
        &BTreeSet::new(),
        Direction::Auto,
    )
    .unwrap()
}
fn accept(conn: &Connection, p: &Plan) {
    decide(
        conn,
        &p.context.installation,
        p,
        1,
        Disposition::Accept,
        "declared reviewer",
        "2026-11-10T00:00:00Z",
    )
    .unwrap();
}

#[test]
fn self_consistent_receipt_never_authorizes_but_actual_local_decision_does() {
    for operation in ["fork", "share", "git"] {
        let mut ctx = context();
        ctx.operation = operation.into();
        let p = plan(&set(""), ctx);
        let conn = Connection::open_in_memory().unwrap();
        assert!(verify(&conn, &p.context.installation, &p).is_err());
        // A file says "accepted" and all its checksums recompute. There is
        // still no authoritative event in the local connection.
        let forged: Plan = serde_json::from_slice(&serde_json::to_vec(&p).unwrap()).unwrap();
        assert_eq!(p.sha256().unwrap(), forged.sha256().unwrap());
        assert!(verify(&conn, &p.context.installation, &forged).is_err());
        accept(&conn, &p);
        let map = verify(&conn, &p.context.installation, &p).unwrap();
        let (ours, theirs) = sides();
        let mut without = ours.clone();
        without.extend(theirs.clone());
        assert_eq!(entities(&without).len(), 2);
        let projected = project(&theirs, &map).unwrap();
        let mut with = ours;
        with.extend(projected);
        assert_eq!(entities(&with).len(), 1);
    }
}

#[test]
fn genuine_acceptance_cannot_be_transplanted_to_changed_contexts_or_rows() {
    let conn = Connection::open_in_memory().unwrap();
    let p = plan(&set(""), context());
    accept(&conn, &p);
    let mut variants = Vec::new();
    for field in 0..7 {
        let mut other = p.clone();
        match field {
            0 => other.context.operation.push('x'),
            1 => other.context.installation.push('x'),
            2 => other.context.base.push('x'),
            3 => other.context.ours.push('x'),
            4 => other.context.theirs.push('x'),
            5 => other.context.scope.push('x'),
            _ => other.context.profile.push('x'),
        }
        variants.push(other);
    }
    let mut other = p.clone();
    other.source_sha256.push('x');
    variants.push(other);
    let mut other = p.clone();
    other.mapping_set_id.push('x');
    variants.push(other);
    let mut other = p.clone();
    other.rows[0].ordinal += 1;
    variants.push(other);
    let mut other = p.clone();
    other.rows[0].row_sha256.push('x');
    variants.push(other);
    let mut other = p.clone();
    other.rows[0].retained.push('x');
    variants.push(other);
    let mut other = p.clone();
    other.rows[0].incoming.push('x');
    variants.push(other);
    let mut other = p.clone();
    other.rows[0].predicate.push('x');
    variants.push(other);
    let mut other = p.clone();
    other.rows[0].direction = Direction::ObjectToSubject;
    variants.push(other);
    for other in variants {
        assert_ne!(p.sha256().unwrap(), other.sha256().unwrap());
        assert!(verify(&conn, &p.context.installation, &other).is_err());
    }
    let clone = Connection::open_in_memory().unwrap();
    assert!(verify(&clone, &p.context.installation, &p).is_err());
}

#[test]
fn latest_local_decision_supersedes_accept_and_unknown_ledger_is_not_empty() {
    let conn = Connection::open_in_memory().unwrap();
    let p = plan(&set(""), context());
    accept(&conn, &p);
    decide(
        &conn,
        &p.context.installation,
        &p,
        1,
        Disposition::Defer,
        "local",
        "later",
    )
    .unwrap();
    let chain: (i64, i64) = conn
        .query_row(
            "SELECT id,supersedes FROM mapping_acceptance_events ORDER BY id DESC LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(chain, (2, 1));
    assert!(verify(&conn, &p.context.installation, &p).is_err());
    decide(
        &conn,
        &p.context.installation,
        &p,
        1,
        Disposition::Reject,
        "local",
        "later",
    )
    .unwrap();
    assert!(
        verify(&conn, &p.context.installation, &p)
            .unwrap()
            .is_empty()
    );
    conn.execute(
        "UPDATE mapping_acceptance_plans SET plan_json='corrupt'",
        [],
    )
    .unwrap();
    assert!(verify(&conn, &p.context.installation, &p).is_err());
}

#[test]
fn nonexact_negated_declined_rows_are_proposals_and_cannot_accept_identity() {
    for (before, after) in [
        ("exactMatch", "broadMatch"),
        ("exactMatch", "narrowMatch"),
        ("exactMatch", "closeMatch"),
    ] {
        let set = ExternalSet::parse(&set("").source.replace(before, after)).unwrap();
        let p = plan(&set, context());
        let conn = Connection::open_in_memory().unwrap();
        assert!(!p.rows[0].identity_eligible);
        assert!(
            decide(
                &conn,
                &p.context.installation,
                &p,
                1,
                Disposition::Accept,
                "local",
                "now"
            )
            .is_err()
        );
        assert!(verify(&conn, &p.context.installation, &p).is_err());
    }
}

#[test]
fn conflicting_targets_and_schema_remaps_refuse_before_acceptance() {
    let text = set("").source;
    let duplicate = text.lines().last().unwrap();
    let conflicting = ExternalSet::parse(&format!(
        "{text}{}\n",
        duplicate.replace("urn:local:a", "urn:local:b")
    ))
    .unwrap();
    let (mut ours, theirs) = sides();
    ours.extend(graph("<urn:local:b> <urn:example:label> \"b\" ."));
    assert!(
        Plan::prepare(
            &conflicting,
            context(),
            &ours,
            &theirs,
            &BTreeSet::new(),
            Direction::Auto
        )
        .is_err()
    );
    let mut schema = BTreeSet::new();
    schema.insert("urn:incoming:a".into());
    assert!(
        Plan::prepare(
            &set(""),
            context(),
            &ours,
            &theirs,
            &schema,
            Direction::Auto
        )
        .is_err()
    );
    let mut theirs = theirs;
    theirs.extend(graph("<urn:other> <urn:incoming:a> \"property\" ."));
    assert!(
        Plan::prepare(
            &set(""),
            context(),
            &ours,
            &theirs,
            &BTreeSet::new(),
            Direction::Auto
        )
        .is_err()
    );
}

#[test]
fn projection_preserves_literals_and_detectable_cardinality_collision() {
    let (ours, mut theirs) = sides();
    theirs.extend(graph("<urn:incoming:a> <urn:example:label> \"b\" .\n<urn:reference> <urn:example:ref> <urn:incoming:a> .\n<urn:reference> <urn:example:text> \"urn:incoming:a\" ."));
    let p = Plan::prepare(
        &set(""),
        context(),
        &ours,
        &theirs,
        &BTreeSet::new(),
        Direction::Auto,
    )
    .unwrap();
    let conn = Connection::open_in_memory().unwrap();
    accept(&conn, &p);
    let projected = project(
        &theirs,
        &verify(&conn, &p.context.installation, &p).unwrap(),
    )
    .unwrap();
    assert!(
        projected
            .iter()
            .any(|t| t.object.to_string() == "<urn:local:a>")
    );
    assert!(
        projected
            .iter()
            .any(|t| t.object.to_string() == "\"urn:incoming:a\"")
    );
    assert_eq!(
        projected
            .iter()
            .filter(|t| t.subject.to_string() == "<urn:local:a>"
                && t.predicate.as_str() == "urn:example:label")
            .count(),
        2
    );
    // Projection never resolves this cardinality collision itself.
}

proptest::proptest! {
    #[test]
    fn any_changed_snapshot_loses_acceptance(suffix in "[a-zA-Z0-9]{1,32}") {
        let conn = Connection::open_in_memory().unwrap();
        let p = plan(&set(""), context()); accept(&conn, &p);
        let mut other = p.clone(); other.context.ours.push_str(&suffix);
        proptest::prop_assert!(verify(&conn, &p.context.installation, &p).is_ok());
        proptest::prop_assert!(verify(&conn, &p.context.installation, &other).is_err());
    }
}

#[test]
fn event_failure_rolls_back_its_new_plan_and_preserves_previous_acceptance() {
    let conn = Connection::open_in_memory().unwrap();
    let p = plan(&set(""), context());
    accept(&conn, &p);
    conn.execute_batch("CREATE TRIGGER refuse_decision BEFORE INSERT ON mapping_acceptance_events BEGIN SELECT RAISE(ABORT,'fixture refusal'); END;").unwrap();
    let mut another = p.clone();
    another.context.ours.push_str("-new");
    assert!(
        decide(
            &conn,
            &another.context.installation,
            &another,
            1,
            Disposition::Accept,
            "local",
            "later"
        )
        .is_err()
    );
    let count: i64 = conn
        .query_row("SELECT count(*) FROM mapping_acceptance_plans", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(count, 1);
    assert!(verify(&conn, &p.context.installation, &p).is_ok());
    assert!(verify(&conn, &another.context.installation, &another).is_err());
}
