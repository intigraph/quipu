//! S1 qualification. Real SQLite files, a real protected journal and
//! inventory, and the real witness logic; nothing here sets a test pin.
use super::*;
use crate::ownership_handoff_model::witness::AuthorityWitness;
use crate::ownership_handoff_model::witness::{Fault, InMemoryWitness};
use crate::ownership_handoff_model::{
    AuthorityUnit, Certificate, Cut, Placement, Request, WitnessHead,
};
use crate::store::{Datum, Store};
use crate::types::{Op, Value};
use std::os::unix::fs::PermissionsExt;

const TS: &str = "2026-11-10T00:00:00Z";
const STORE_ID: &str = "urn:example:s1";
const PROFILE: &str = "fixture-witness";

fn placement(name: &str, byte: u8) -> Placement {
    Placement {
        principal: name.into(),
        native: [byte; 32],
    }
}

fn authority() -> Authority {
    Authority {
        store_id: STORE_ID.into(),
        unit: AuthorityUnit::FullStore,
        generation: 0,
        epoch: "epoch-A".into(),
        owner: placement("A", 1),
        policy: "existing-policy".into(),
        predecessor: None,
    }
}

fn certificate(op: &str) -> Certificate {
    Certificate {
        request: Request {
            operation: op.into(),
            operator: "existing-operator".into(),
            successor_generation: 1,
            source: authority(),
            destination: placement("B", 2),
            profile: "existing-profile".into(),
        },
        cut: Cut {
            transaction: 1,
            facts: [3; 32],
            artifact: [4; 32],
        },
    }
}

struct Source {
    witness: InMemoryWitness,
}

impl WitnessSource for Source {
    fn head_evidence(
        &self,
        profile: &str,
        store_id: &str,
        anchor: u64,
    ) -> Reply<(WitnessHead, Vec<(String, String)>)> {
        if profile != PROFILE {
            return Reply::Unknown;
        }
        self.witness.head_evidence(store_id, anchor)
    }
}

struct Fixture {
    _dir: tempfile::TempDir,
    db: PathBuf,
    journal: PathBuf,
    inventory: PathBuf,
    source: Source,
}

fn mode(path: &Path, mode: u32) {
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
}

impl Fixture {
    fn new() -> Self {
        Self::with_setup(|db| drop(Store::open(db).unwrap()))
    }

    /// `setup` prepares the ordinary database before enrollment (D4 order).
    fn with_setup(setup: impl FnOnce(&str)) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("store.db");
        setup(db.to_str().unwrap());
        let protected = dir.path().join("protected");
        fs::create_dir(&protected).unwrap();
        mode(&protected, 0o700);
        let journal = protected.join("lineage.journal");
        ownership_journal::create(
            &journal,
            &Journal::Active {
                authority: authority(),
            },
        )
        .unwrap();
        mode(&journal, 0o600);
        let inventory = protected.join("inventory");
        let Reply::Ok(witness) = InMemoryWitness::new(authority()) else {
            panic!("genesis");
        };
        let f = Self {
            _dir: dir,
            db,
            journal,
            inventory,
            source: Source { witness },
        };
        f.enroll();
        f
    }

    fn actual(&self) -> Database {
        database(&self.db).unwrap().1
    }

    fn entry(&self) -> inventory::Entry {
        inventory::Entry {
            store_id: STORE_ID.into(),
            unit: AuthorityUnit::FullStore,
            database: self.actual(),
            journal: self.journal.to_str().unwrap().into(),
            witness: PROFILE.into(),
            enrolled_at_generation: 0,
        }
    }

    /// Private fixture enrollment: inventory record, then marker (D4 order).
    fn enroll(&self) {
        let entry = self.entry();
        let digest = entry.digest().unwrap();
        let bytes = inventory::Inventory {
            version: 1,
            records: vec![entry],
        }
        .encode()
        .unwrap();
        fs::write(&self.inventory, bytes).unwrap();
        mode(&self.inventory, 0o600);
        let binding = Binding {
            version: 1,
            store_id: STORE_ID.into(),
            inventory_digest: digest,
        };
        let conn = Connection::open(&self.db).unwrap();
        conn.execute_batch(MARKER_SQL).unwrap();
        conn.execute(
            "INSERT INTO ownership_handoff_enrollment(id, binding) VALUES (1, ?1)",
            [serde_json::to_string(&binding).unwrap()],
        )
        .unwrap();
    }

    fn config(&self) -> EnrolledStartup<'_> {
        EnrolledStartup {
            inventory: &self.inventory,
            witness: &self.source,
        }
    }

    fn open(&self) -> Result<Store> {
        Store::open_enrolled(self.db.to_str().unwrap(), &self.config())
    }

    fn receipt(&self) -> PathBuf {
        ownership_latch::sibling(&self.journal, ".startup-receipt").unwrap()
    }
}

fn write(store: &mut Store, tag: &str) -> Result<i64> {
    let datum = Datum {
        entity: store.intern(&format!("urn:s1:{tag}"))?,
        attribute: store.intern("urn:s1:p")?,
        value: Value::Int(1),
        valid_from: TS.into(),
        valid_to: None,
        op: Op::Assert,
    };
    store.transact(&[datum], TS, None, None)
}

fn refused_without_receipt(f: &Fixture, why: &str) {
    match f.open() {
        Err(Error::PolicyDenied(msg)) => {
            assert!(msg.contains("enrolled startup refused"), "{why}: {msg}");
        }
        Err(other) => panic!("{why}: unexpected error {other}"),
        Ok(_) => panic!("{why}: admitted"),
    }
    assert!(!f.receipt().exists(), "{why}: no receipt on refusal");
}

#[test]
fn admitted_owner_writes_through_the_seam_and_records_a_receipt() {
    let f = Fixture::new();
    // The ordinary route still refuses the enrolled database.
    assert!(
        Store::open(f.db.to_str().unwrap()).is_err(),
        "ordinary open refuses"
    );
    let mut store = f.open().expect("S1 admission");
    store.ensure_ownership_writable().unwrap();
    store
        .ownership_operation(|s| write(s, "admitted"))
        .expect("admitted write through the P1 seam");
    // A pinned write outside the seam is refused, as before.
    assert!(
        write(&mut store, "outside-seam").is_err(),
        "outside the seam"
    );
    let receipt: serde_json::Value =
        serde_json::from_slice(&fs::read(f.receipt()).unwrap()).unwrap();
    assert_eq!(receipt["version"], 1);
    assert_eq!(receipt["witness_profile"], PROFILE);
    assert_eq!(receipt["authority"]["generation"], 0);
    assert_eq!(receipt["nonce"].as_str().unwrap().len(), 64);
    assert!(!receipt["witness_versions"].as_array().unwrap().is_empty());
    assert_eq!(receipt["database"]["inode"], f.actual().inode);
    assert!(
        !String::from_utf8(fs::read(f.receipt()).unwrap())
            .unwrap()
            .contains("secret"),
        "no credential material"
    );
}

#[test]
fn every_startup_draws_a_fresh_nonce() {
    let f = Fixture::new();
    drop(f.open().unwrap());
    let first: serde_json::Value = serde_json::from_slice(&fs::read(f.receipt()).unwrap()).unwrap();
    drop(f.open().unwrap());
    let second: serde_json::Value =
        serde_json::from_slice(&fs::read(f.receipt()).unwrap()).unwrap();
    assert_ne!(first["nonce"], second["nonce"]);
}

#[test]
fn witness_doubt_refuses_startup() {
    let f = Fixture::new();
    for fault in [Fault::Unknown, Fault::Slow] {
        assert_eq!(f.source.witness.inject(fault), Reply::Ok(()));
        refused_without_receipt(&f, &format!("{fault:?}"));
    }
}

#[test]
fn reserved_witness_refuses_ordinary_startup() {
    let f = Fixture::new();
    assert_eq!(
        f.source.witness.reserve(0, &certificate("op1").request),
        Reply::Ok(())
    );
    refused_without_receipt(&f, "reserved");
}

#[test]
fn restored_old_journal_with_witness_ahead_refuses() {
    let f = Fixture::new();
    let c = certificate("op1");
    assert_eq!(f.source.witness.reserve(0, &c.request), Reply::Ok(()));
    assert_eq!(f.source.witness.retire(0, &c), Reply::Ok(()));
    refused_without_receipt(&f, "retired ahead");
    assert_eq!(
        f.source.witness.activate(&c.successor("epoch-B").unwrap()),
        Reply::Ok(())
    );
    refused_without_receipt(&f, "successor active ahead");
}

#[test]
fn non_active_journal_refuses() {
    let f = Fixture::new();
    let c = certificate("op1");
    ownership_journal::advance(
        &f.journal,
        &Journal::Active {
            authority: authority(),
        },
        &Journal::Retiring {
            request: c.request.clone(),
        },
    )
    .unwrap();
    refused_without_receipt(&f, "retiring journal");
}

#[test]
fn copied_or_linked_database_refuses() {
    let f = Fixture::new();
    let copy = f.db.with_file_name("copy.db");
    fs::copy(&f.db, &copy).unwrap();
    assert!(
        Store::open_enrolled(copy.to_str().unwrap(), &f.config()).is_err(),
        "copy has a different inode"
    );
    let link = f.db.with_file_name("link.db");
    fs::hard_link(&f.db, &link).unwrap();
    refused_without_receipt(&f, "hard link");
}

#[test]
fn unprotected_inventory_or_journal_refuses() {
    let f = Fixture::new();
    mode(&f.inventory, 0o620);
    refused_without_receipt(&f, "group-writable inventory");
    mode(&f.inventory, 0o600);
    mode(&f.journal, 0o640);
    refused_without_receipt(&f, "group-readable journal");
}

#[test]
fn marker_digest_mismatch_refuses() {
    let f = Fixture::new();
    let conn = Connection::open(&f.db).unwrap();
    let wrong = Binding {
        version: 1,
        store_id: STORE_ID.into(),
        inventory_digest: "0".repeat(64),
    };
    conn.execute(
        "UPDATE ownership_handoff_enrollment SET binding=?1 WHERE id=1",
        [serde_json::to_string(&wrong).unwrap()],
    )
    .unwrap();
    refused_without_receipt(&f, "marker digest mismatch");
}

#[test]
fn unknown_marker_schema_refuses() {
    let f = Fixture::new();
    let conn = Connection::open(&f.db).unwrap();
    conn.execute_batch("DROP TABLE ownership_handoff_enrollment; CREATE TABLE ownership_handoff_enrollment(binding TEXT)")
        .unwrap();
    refused_without_receipt(&f, "noncanonical marker schema");
}

#[test]
fn receipt_durability_failure_means_no_handle() {
    let f = Fixture::new();
    fs::create_dir(f.receipt()).unwrap();
    match f.open() {
        Err(Error::PolicyDenied(msg)) => assert!(msg.contains("receipt"), "{msg}"),
        other => panic!("receipt failure admitted: {:?}", other.map(|_| ())),
    }
}

#[test]
fn changed_marker_after_admission_revokes_for_good() {
    let f = Fixture::new();
    let mut store = f.open().unwrap();
    let raw = Connection::open(&f.db).unwrap();
    let saved: String = raw
        .query_row(
            "SELECT binding FROM ownership_handoff_enrollment",
            [],
            |r| r.get(0),
        )
        .unwrap();
    raw.execute("DELETE FROM ownership_handoff_enrollment", [])
        .unwrap();
    assert!(store.ensure_ownership_writable().is_err(), "marker gone");
    raw.execute(
        "INSERT INTO ownership_handoff_enrollment(id, binding) VALUES (1, ?1)",
        [saved],
    )
    .unwrap();
    assert!(
        store.ensure_ownership_writable().is_err(),
        "restoring does not revive"
    );
    assert!(store.ownership_operation(|s| write(s, "revoked")).is_err());
}

#[test]
fn running_owner_survives_a_witness_outage() {
    let f = Fixture::new();
    let mut store = f.open().unwrap();
    for _ in 0..4 {
        assert_eq!(f.source.witness.inject(Fault::Unknown), Reply::Ok(()));
    }
    store
        .ownership_operation(|s| write(s, "during-outage"))
        .expect("the witness is not consulted per commit");
    store.ensure_ownership_writable().unwrap();
}

#[test]
fn journal_retirement_after_admission_revokes_the_handle() {
    let f = Fixture::new();
    let mut store = f.open().unwrap();
    ownership_journal::advance(
        &f.journal,
        &Journal::Active {
            authority: authority(),
        },
        &Journal::Retiring {
            request: certificate("op1").request,
        },
    )
    .unwrap();
    assert!(
        store
            .ownership_operation(|s| write(s, "after-retiring"))
            .is_err()
    );
    assert!(store.ensure_ownership_writable().is_err());
}

#[test]
fn unknown_profile_refuses() {
    let f = Fixture::new();
    let other = Source {
        witness: {
            let Reply::Ok(w) = InMemoryWitness::new(authority()) else {
                panic!()
            };
            w
        },
    };
    struct WrongProfile<'a>(&'a Source);
    impl WitnessSource for WrongProfile<'_> {
        fn head_evidence(
            &self,
            _: &str,
            store_id: &str,
            anchor: u64,
        ) -> Reply<(WitnessHead, Vec<(String, String)>)> {
            self.0
                .head_evidence("not-the-inventory-profile", store_id, anchor)
        }
    }
    let config = EnrolledStartup {
        inventory: &f.inventory,
        witness: &WrongProfile(&other),
    };
    assert!(Store::open_enrolled(f.db.to_str().unwrap(), &config).is_err());
    assert!(!f.receipt().exists());
}

#[test]
fn startup_refuses_while_a_transition_holds_the_lineage_latch() {
    let f = Fixture::new();
    let transition = ownership_latch::exclusive(&f.journal).unwrap();
    refused_without_receipt(&f, "exclusive transition latch held");
    drop(transition);
    drop(
        f.open()
            .expect("admits once the transition releases the latch"),
    );
}

/// One positive fixture through the same non-test route over the W2 backend
/// and its private conditional-create emulator, plus a transport refusal.
#[cfg(feature = "ownership-witness-s3")]
#[test]
fn admission_through_the_s3_witness_backend_and_emulator() {
    use crate::ownership_handoff_model::witness::objects::{Created, Objects};
    use crate::ownership_handoff_model::witness::s3::S3Objects;
    use crate::ownership_handoff_model::witness::s3_emulator::{EmuFault, Emulator};
    use crate::ownership_handoff_model::witness::{ObjectWitness, assumed_genesis_bytes};

    struct S3Source(ObjectWitness<S3Objects>);
    impl WitnessSource for S3Source {
        fn head_evidence(
            &self,
            profile: &str,
            store_id: &str,
            anchor: u64,
        ) -> Reply<(WitnessHead, Vec<(String, String)>)> {
            if profile != PROFILE {
                return Reply::Unknown;
            }
            self.0.head_evidence(store_id, anchor)
        }
    }

    let f = Fixture::new();
    let emulator = Emulator::start();
    let backend = emulator.backend();
    assert_eq!(
        backend.create("active/00", &assumed_genesis_bytes(&authority())),
        Reply::Ok(Created::New)
    );
    let source = S3Source(ObjectWitness::attach(STORE_ID.into(), backend));
    let config = EnrolledStartup {
        inventory: &f.inventory,
        witness: &source,
    };
    let mut store =
        Store::open_enrolled(f.db.to_str().unwrap(), &config).expect("admitted over S3");
    store
        .ownership_operation(|s| write(s, "over-s3"))
        .expect("admitted write");
    let receipt: serde_json::Value =
        serde_json::from_slice(&fs::read(f.receipt()).unwrap()).unwrap();
    assert!(
        receipt["witness_versions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v[0] == "active/00" && v[1] == "\"v1\""),
        "S3 entity tag recorded: {receipt}"
    );
    drop(store);
    fs::remove_file(f.receipt()).unwrap();
    emulator.push(EmuFault::Status500);
    refused_without_receipt_with(&f, &config, "S3 server error");
}

#[cfg(feature = "ownership-witness-s3")]
fn refused_without_receipt_with(f: &Fixture, config: &EnrolledStartup<'_>, why: &str) {
    assert!(
        Store::open_enrolled(f.db.to_str().unwrap(), config).is_err(),
        "{why}: admitted"
    );
    assert!(!f.receipt().exists(), "{why}: no receipt on refusal");
}

/// 4qv: the S1 route is a writable route, so it must take the same shared
/// native writer lease as ordinary open. Otherwise an enrolled handle could
/// write while a graph Owner holds exclusive admission on the same file.
#[test]
fn enrolled_handle_takes_the_native_writer_lease() {
    use rustix::fs::{FlockOperation, flock};
    let f = Fixture::new();
    // The lease is the sibling file; never open the database outside SQLite.
    let lease = ownership_latch::sibling(&f.db, ".writer-lease").unwrap();
    let exclusive = || {
        fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&lease)
            .unwrap()
    };

    // Exclusive admission held elsewhere (what Owner::prepare takes): S1 refuses.
    let owner = exclusive();
    flock(&owner, FlockOperation::NonBlockingLockExclusive).unwrap();
    assert!(
        f.open().is_err(),
        "S1 open while exclusive admission is held"
    );
    drop(owner);

    // An admitted enrolled handle holds the shared lease, so exclusive
    // admission is refused until the handle is gone.
    let store = f.open().expect("S1 admission");
    assert!(
        store.writer_lease.is_some(),
        "enrolled handle holds a lease"
    );
    let probe = exclusive();
    assert!(
        flock(&probe, FlockOperation::NonBlockingLockExclusive).is_err(),
        "exclusive admission while an enrolled handle is open"
    );
    drop(store);
    flock(&probe, FlockOperation::NonBlockingLockExclusive)
        .expect("exclusive admission after the enrolled handle drops");
}

#[path = "ownership_admission_owner_tests.rs"]
mod graph_owner_startup;
