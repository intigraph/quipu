//! aegis-4qv.4 exit proof: an enrolled pilot owner, reversed to legacy ROOT,
//! reads back RETIRED in both the witness head and the journal (never
//! `active_unretired`), over the S3 emulator. Sabotage: an enroll-tagged ROOT
//! placement and a profile mismatch refuse with nothing written. Faults:
//! UNKNOWN and `LostReply` on reserve and on retire stop, and a re-run resumes
//! to the identical receipt.
use super::*;
use crate::ownership_handoff_model::AuthorityUnit;
use crate::ownership_handoff_model::witness::s3::S3Objects;
use crate::ownership_handoff_model::witness::s3_emulator::{EmuFault, Emulator};
use crate::store::Store;
use crate::store::ownership_enroll::{EnrollRequest, EnrollWitness, enroll_owner, native_digest};
use std::path::PathBuf;

const PROFILE: &str = "pilot-test-witness";

struct Fixture {
    _dir: tempfile::TempDir,
    journal: PathBuf,
    legacy: PathBuf,
    emulator: Emulator,
    backend: S3Objects,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("owner.db");
        let legacy = dir.path().join("legacy.db");
        drop(Store::open(db.to_str().unwrap()).unwrap());
        drop(Store::open(legacy.to_str().unwrap()).unwrap());
        let journal = dir.path().join("owner.journal");
        let emulator = Emulator::start();
        let backend = emulator.backend();
        enroll_owner(&EnrollRequest {
            db_path: &db,
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
        Self {
            _dir: dir,
            journal,
            legacy,
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
            profile,
            operator: "pilot-operator",
            destination,
            legacy_db: &self.legacy,
            cut: Cut {
                transaction: 1,
                facts: [3; 32],
                artifact: [4; 32],
            },
            witness_profile: PROFILE,
            objects: &self.backend,
        }
    }
    fn head(&self, store_id: &str) -> WitnessHead {
        match ObjectWitness::attach(store_id.into(), &self.backend).head(store_id, 0) {
            Reply::Ok(head) => head,
            other => panic!("head: {other:?}"),
        }
    }
    fn journal(&self) -> Journal {
        match ownership_journal::read(&self.journal) {
            JournalState::Known(j) => *j,
            JournalState::Unknown(why) => panic!("journal: {why}"),
        }
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

#[test]
fn reversed_pilot_owner_reads_back_retired_not_active_unretired() {
    let f = Fixture::new();
    let store_id = match f.journal() {
        Journal::Active { authority } => authority.store_id,
        other => panic!("enrolled journal: {other:?}"),
    };
    assert!(matches!(f.head(&store_id), WitnessHead::Active { .. }));
    let planned = plan_retirement(&f.request(PROFILE, f.root())).expect("dry run");
    assert_eq!(
        f.listing(),
        vec!["active/00".to_string()],
        "dry run wrote nothing"
    );
    assert_eq!(planned.successor_generation, planned.source.generation + 1);

    let receipt = retire_owner(&f.request(PROFILE, f.root())).expect("retired");
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
    assert_eq!(
        retire_owner(&f.request(PROFILE, f.root())).unwrap(),
        receipt
    );
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
    let f = Fixture::new();
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
/// Per-transition UNKNOWN/LostReply on retire and abandon is covered at the
/// witness level by the shared conformance suite, over both backends.
#[test]
fn unknown_and_lost_reply_stop_then_resume() {
    // (case, reserve directly first, fault, expected witness phase after stop)
    let cases = [
        (
            "unknown before reserve",
            false,
            EmuFault::Status500,
            "ACTIVE",
        ),
        (
            "lost reply on reserve",
            false,
            EmuFault::LostReplyOnPut,
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
        let f = Fixture::new();
        if reserve_first {
            // Reserve through the witness, so the run's only PUT is retire.
            let request = plan_retirement(&f.request(PROFILE, f.root())).unwrap();
            let w = ObjectWitness::attach(request.source.store_id.clone(), &f.backend);
            assert_eq!(w.reserve(ATTEMPT, &request), Reply::Ok(()), "{name}");
        }
        f.emulator.push(fault);
        let err = retire_owner(&f.request(PROFILE, f.root()))
            .unwrap_err()
            .to_string();
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
        let receipt = retire_owner(&f.request(PROFILE, f.root())).expect(name);
        f.assert_retired(&receipt);
        assert!(receipt.operation.starts_with("retire-to-legacy-root/"));
    }
}
