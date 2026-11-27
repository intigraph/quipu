//! aegis-4qv.4 exit proof: an enrolled pilot owner, reversed to legacy ROOT,
//! reads back RETIRED in both the witness head and the journal (never
//! `active_unretired`), over the S3 emulator. R4: one refusal per clause of
//! "reversed and unrouted" (not reversed, reverse only Proven, catalog R+1
//! still routes the owner, owner lock held), plus an owner that changed after
//! its reverse. R5: the cut is derived from the owner file, and a re-run with
//! a different derived cut refuses. Sabotage: an enroll-tagged ROOT placement
//! and a profile mismatch refuse with nothing written. Faults: UNKNOWN (500)
//! and `LostReply` on the reserve PUT and on the retire PUT stop, and a re-run
//! resumes to RETIRED.
use super::*;
use crate::graph_catalog::{Budget, Route};
use crate::ownership_handoff_model::AuthorityUnit;
use crate::ownership_handoff_model::witness::s3::S3Objects;
use crate::ownership_handoff_model::witness::s3_emulator::{EmuFault, Emulator};
use crate::store::ownership_enroll::{EnrollRequest, EnrollWitness, enroll_owner};
use crate::store::{ReversePlan, Store};
use serde_json::json;
use std::path::PathBuf;

const PROFILE: &str = "pilot-test-witness";
const GRAPH: &str = "https://example.test/graphs/ingest";
const TAG: &str = "snapshot:code:repo";
const TS: &str = "2026-11-11T00:00:00Z";
const AT: &str = "2026-11-26T20:00:00Z";
const CODE: &str = "<urn:rev:file> <urn:rev:defines> <urn:rev:symbol> .";
const PLAN: ReversePlan<'static> = ReversePlan {
    graph: GRAPH,
    catalog_revision: 7,
    generation: 4,
};

fn knot(store: &mut Store, input: &serde_json::Value) {
    crate::tool_knot(store, input).unwrap();
}

fn catalog(revision: u64, routes: Vec<Route>) -> Catalog {
    let budget = Budget {
        max_owners: 2,
        queued_per_owner: 1,
        readers_per_owner: 1,
        max_queued: 2,
        max_connections: 6,
    };
    Catalog::new(revision, "legacy-store", routes, budget).unwrap()
}

struct Fixture {
    _dir: tempfile::TempDir,
    journal: PathBuf,
    owner: PathBuf,
    legacy: PathBuf,
    current: Catalog,
    next: Catalog,
    emulator: Emulator,
    backend: S3Objects,
}

impl Fixture {
    /// Enroll -> route at (7, 4) -> owner write -> `steps` reverse steps
    /// (3 = Lifted). Catalog R+1 routes nothing.
    fn new(steps: usize) -> Self {
        Self::with(steps, |_| {})
    }
    /// As [`Self::new`], with `late` run against the owner after the reverse
    /// steps and before enrollment (an enrolled file refuses a writable open).
    fn with(steps: usize, late: impl FnOnce(&mut Store)) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let (owner, legacy) = (root.join("owner.db"), root.join("legacy.db"));
        let journal = root.join("owner.journal");
        let mut l = Store::open(legacy.to_str().unwrap()).unwrap();
        knot(
            &mut l,
            &json!({"turtle": CODE, "replace_snapshot": true, "snapshot": "code:repo", "source": "t"}),
        );
        let slice = l
            .export_routed_slice(TAG, crate::schema::ROOT_GRAPH_IRI)
            .unwrap();
        let mut o = Store::open(owner.to_str().unwrap()).unwrap();
        o.graph_create(GRAPH).unwrap();
        o.import_routed_slice(&slice, TAG, GRAPH, TS).unwrap();
        l.fence_routed_graph(GRAPH, 7, 4).unwrap();
        l.cut_over_routed_source(TAG, GRAPH, 7, 4, TS).unwrap();
        knot(
            &mut o,
            &json!({"turtle": "<urn:rev:w> <urn:rev:n> \"1\" .", "graph": GRAPH}),
        );
        drop(o);
        let lock = lock_owner_file(owner.to_str().unwrap()).unwrap();
        let facts = lock.export_routed_graph(GRAPH).unwrap();
        for _ in 0..steps {
            l.reverse_step(&PLAN, &facts, AT).unwrap();
        }
        drop((lock, l));
        late(&mut Store::open(owner.to_str().unwrap()).unwrap());
        let emulator = Emulator::start();
        let backend = emulator.backend();
        enroll_owner(&EnrollRequest {
            db_path: &owner,
            journal_path: &journal,
            unit: AuthorityUnit::FullStore,
            profile: PROFILE,
            principal: "pilot-owner",
            policy: "pilot-policy",
            witness: EnrollWitness {
                profile: PROFILE,
                objects: &backend,
            },
        })
        .expect("enrolled");
        let Journal::Active { authority } = read_journal(&journal) else {
            panic!("enrolled journal is not Active")
        };
        let route = Route {
            graph: GRAPH.into(),
            file_id: "owner".into(),
            store_id: authority.store_id.clone(),
            log_epoch: authority.epoch.clone(),
            generation: 4,
            policy: "pilot-policy".into(),
        };
        Self {
            _dir: dir,
            journal,
            owner,
            legacy,
            current: catalog(7, vec![route]),
            next: catalog(8, vec![]),
            emulator,
            backend,
        }
    }
    fn root(&self) -> Placement {
        let (_, db) = database(&self.legacy).unwrap();
        legacy_root_placement("legacy-root-writer", &db)
    }
    fn request(
        &self,
        profile: &'static str,
        destination: Placement,
    ) -> RetireRequest<'_, S3Objects> {
        RetireRequest {
            journal_path: &self.journal,
            owner_db: &self.owner,
            current: &self.current,
            next: &self.next,
            profile,
            operator: "pilot-operator",
            destination,
            legacy_db: &self.legacy,
            witness_profile: PROFILE,
            objects: &self.backend,
        }
    }
    fn ok(&self) -> RetireRequest<'_, S3Objects> {
        self.request(PROFILE, self.root())
    }
    fn head(&self, store_id: &str) -> WitnessHead {
        match ObjectWitness::attach(store_id.into(), &self.backend).head(store_id, 0) {
            Reply::Ok(head) => head,
            other => panic!("head: {other:?}"),
        }
    }
    fn journal(&self) -> Journal {
        read_journal(&self.journal)
    }
    fn listing(&self) -> Vec<String> {
        match self.backend.list(64) {
            Reply::Ok(keys) => keys,
            other => panic!("listing: {other:?}"),
        }
    }
    fn assert_retired(&self, receipt: &RetireReceipt) {
        assert!(matches!(
            self.head(&receipt.store_id),
            WitnessHead::Retired { .. }
        ));
        assert!(matches!(self.journal(), Journal::Retired { .. }));
        assert_eq!(receipt.witness_state, "RETIRED");
        assert!(!receipt.signed, "never claims a signature");
    }
}

fn read_journal(path: &Path) -> Journal {
    match ownership_journal::read(path) {
        JournalState::Known(j) => *j,
        JournalState::Unknown(why) => panic!("journal: {why}"),
    }
}

fn refusal(req: &RetireRequest<'_, S3Objects>) -> String {
    retire_owner(req).unwrap_err().to_string()
}

#[test]
fn reversed_pilot_owner_reads_back_retired_not_active_unretired() {
    let f = Fixture::new(3);
    let store_id = match f.journal() {
        Journal::Active { authority } => authority.store_id,
        other => panic!("enrolled journal: {other:?}"),
    };
    assert!(matches!(f.head(&store_id), WitnessHead::Active { .. }));
    let planned = plan_retirement(&f.ok()).expect("dry run");
    assert_eq!(
        f.listing(),
        vec!["active/00".to_string()],
        "dry run wrote nothing"
    );
    let request = &planned.request;
    assert_eq!(request.successor_generation, request.source.generation + 1);
    assert_eq!(planned.reversed_graphs, vec![GRAPH.to_string()]);
    assert!(
        planned.cut.transaction > 0,
        "cut derived from the owner file"
    );

    let receipt = retire_owner(&f.ok()).expect("retired");
    f.assert_retired(&receipt);
    assert_eq!(
        f.listing(),
        vec![
            "active/00".to_string(),
            "outcome/00/00".to_string(),
            "reserved/00/00".to_string()
        ],
        "reserve then retire, and no activate"
    );
    // Exact re-run is idempotent; the Retired record is terminal.
    assert_eq!(retire_owner(&f.ok()).unwrap(), receipt);
    // Admission refuses the retired handle by role.
    let Journal::Retired { certificate } = f.journal() else {
        unreachable!()
    };
    let err = ownership_journal::admits(&f.journal, &certificate.request.source)
        .unwrap_err()
        .to_string();
    assert!(err.contains("RETIRED"), "{err}");
}

#[test]
fn enroll_tagged_root_placement_and_profile_mismatch_refuse_without_writes() {
    let f = Fixture::new(3);
    let before = (f.listing(), f.journal());
    let (_, legacy) = database(&f.legacy).unwrap();
    let enroll_tagged = Placement {
        principal: "legacy-root-writer".into(),
        native: native_digest(&legacy),
    };
    let err = retire_owner(&f.request(PROFILE, enroll_tagged))
        .unwrap_err()
        .to_string();
    assert!(err.contains("enrollment domain tag"), "{err}");
    let foreign = Placement {
        principal: "legacy-root-writer".into(),
        native: [9; 32],
    };
    let err = retire_owner(&f.request(PROFILE, foreign))
        .unwrap_err()
        .to_string();
    assert!(err.contains("not the legacy ROOT placement"), "{err}");
    let err = retire_owner(&f.request("other-profile", f.root()))
        .unwrap_err()
        .to_string();
    assert!(err.contains("profile differs"), "{err}");
    assert_eq!((f.listing(), f.journal()), before, "refusals wrote nothing");
}

/// UNKNOWN and `LostReply` stop as UNKNOWN, leave a state the model accepts
/// (never absence, never a skipped phase), and a re-run resumes to RETIRED.
/// "unknown on retire" stops with witness Reserved and journal Retiring and
/// resumes from there. Per-transition faults on abandon are covered at the
/// witness level by the shared conformance suite, over both backends.
#[test]
fn unknown_and_lost_reply_stop_then_resume() {
    // (case, reserve directly first, fault, expected witness phase after stop)
    let cases = [
        ("unknown on head", false, EmuFault::Status500, "ACTIVE"),
        (
            "unknown on reserve",
            false,
            EmuFault::Status500OnPut,
            "ACTIVE",
        ),
        (
            "lost reply on reserve",
            false,
            EmuFault::LostReplyOnPut,
            "RESERVED",
        ),
        (
            "unknown on retire",
            true,
            EmuFault::Status500OnPut,
            "RESERVED",
        ),
        (
            "lost reply on retire",
            true,
            EmuFault::LostReplyOnPut,
            "RETIRED",
        ),
    ];
    for (name, reserve_first, fault, phase) in cases {
        let f = Fixture::new(3);
        if reserve_first {
            // Reserve through the witness, so the run's only PUT is retire.
            let request = plan_retirement(&f.ok()).unwrap().request;
            let w = ObjectWitness::attach(request.source.store_id.clone(), &f.backend);
            assert_eq!(w.reserve(ATTEMPT, &request), Reply::Ok(()), "{name}");
        }
        f.emulator.push(fault);
        let err = retire_owner(&f.ok()).unwrap_err().to_string();
        assert!(err.contains("UNKNOWN"), "{name}: {err}");
        let id = match f.journal() {
            Journal::Active { authority } => authority.store_id,
            Journal::Retiring { request } => request.source.store_id,
            other => panic!("{name}: journal skipped to {other:?}"),
        };
        let found = match f.head(&id) {
            WitnessHead::Active { .. } => "ACTIVE",
            WitnessHead::Reserved { .. } => "RESERVED",
            WitnessHead::Retired { .. } => "RETIRED",
        };
        assert_eq!(found, phase, "{name}");
        let receipt = retire_owner(&f.ok()).expect(name);
        f.assert_retired(&receipt);
        assert!(receipt.operation.starts_with("retire-to-legacy-root/"));
    }
}

/// R4: one refusal per clause, each before any witness call or local write.
#[test]
fn only_a_reversed_unrouted_owner_is_retired() {
    let check = |f: &Fixture, req: &RetireRequest<'_, S3Objects>, needle: &str| {
        let before = (f.listing(), f.journal());
        let err = refusal(req);
        assert!(err.contains(needle), "{needle}: {err}");
        assert_eq!((f.listing(), f.journal()), before, "{needle}: wrote");
    };
    // (b) a live routed owner: no reverse journaled at all.
    let live = Fixture::new(0);
    check(&live, &live.ok(), "no journaled reverse");
    // (b) the reverse stopped at Proven.
    let proven = Fixture::new(2);
    check(&proven, &proven.ok(), "reverse is proven, not lifted");
    // (c) catalog R+1 still routes the owner.
    let mut routed = Fixture::new(3);
    let still: Vec<Route> = routed.current.routes().cloned().collect();
    routed.next = catalog(8, still);
    check(&routed, &routed.ok(), "still routes");
    // (a) another process holds the owner file.
    let held = Fixture::new(3);
    let other = lock_owner_file(held.owner.to_str().unwrap()).unwrap();
    check(&held, &held.ok(), "in use");
    drop(other);
    // Cross-check: the owner wrote after its reverse.
    let changed = Fixture::with(3, |o| {
        knot(
            o,
            &json!({"turtle": "<urn:rev:late> <urn:rev:n> \"2\" .", "graph": GRAPH}),
        );
    });
    check(&changed, &changed.ok(), "owner changed after reverse");
    // Positive control: the same fixture, reversed and unrouted, retires.
    f_retires(&Fixture::new(3));
}

fn f_retires(f: &Fixture) {
    let receipt = retire_owner(&f.ok()).expect("retired");
    f.assert_retired(&receipt);
}

/// R5 idempotency: a re-run re-derives the cut; once the witness records a
/// certificate, a different derived cut refuses instead of reporting Ok.
#[test]
fn a_different_cut_after_retired_refuses() {
    let f = Fixture::new(3);
    f_retires(&f);
    // Any byte of the owner image changes the derived artifact digest, while
    // the routed graph's multiset (the reverse cross-check) is untouched.
    rusqlite::Connection::open(&f.owner)
        .unwrap()
        .execute_batch("PRAGMA user_version = 99;")
        .unwrap();
    let err = refusal(&f.ok());
    assert!(err.contains("different cut"), "{err}");
}
