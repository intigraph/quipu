//! aegis-4qv.2 controls: enrollment over the S3 witness backend and its
//! private conditional-create emulator, refusals that leave the database,
//! inventory and witness unchanged, and crash-resume after steps 3, 4 and 5.
use super::*;
use crate::ownership_handoff_model::witness::s3::S3Objects;
use crate::ownership_handoff_model::witness::s3_emulator::{EmuFault, Emulator};
use std::os::unix::fs::PermissionsExt;

const PROFILE: &str = "pilot-test-witness";

/// db hash, inventory bytes, journal bytes, witness listing.
type Snapshot = (String, Option<Vec<u8>>, Option<Vec<u8>>, Vec<String>);

struct Fixture {
    dir: tempfile::TempDir,
    db: PathBuf,
    journal: PathBuf,
    emulator: Emulator,
    backend: S3Objects,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("owner.db");
        drop(Store::open(db.to_str().unwrap()).unwrap());
        let journal = dir.path().join("owner.journal");
        let emulator = Emulator::start();
        let backend = emulator.backend();
        Self {
            dir,
            db,
            journal,
            emulator,
            backend,
        }
    }
    fn request(&self) -> EnrollRequest<'_, S3Objects> {
        self.request_with(PROFILE)
    }
    fn request_with<'a>(&'a self, profile: &'a str) -> EnrollRequest<'a, S3Objects> {
        EnrollRequest {
            db_path: &self.db,
            journal_path: &self.journal,
            unit: AuthorityUnit::FullStore,
            profile,
            principal: "pilot-owner",
            policy: "pilot-policy",
            witness: EnrollWitness {
                profile: PROFILE,
                objects: &self.backend,
            },
        }
    }
    fn inventory(&self) -> PathBuf {
        self.journal.with_extension("inventory")
    }
    fn intent(&self) -> PathBuf {
        self.journal.with_extension("enroll-intent")
    }
    fn listing(&self) -> Vec<String> {
        match self.backend.list(64) {
            Reply::Ok(keys) => keys,
            other => panic!("listing: {other:?}"),
        }
    }
    /// db hash, inventory bytes, journal bytes, witness listing.
    fn snapshot(&self) -> Snapshot {
        (
            hex::encode(Sha256::digest(fs::read(&self.db).unwrap())),
            fs::read(self.inventory()).ok(),
            fs::read(&self.journal).ok(),
            self.listing(),
        )
    }
    fn refuses(&self, req: &EnrollRequest<'_, S3Objects>, why: &str) {
        let before = self.snapshot();
        let err = enroll_owner(req).expect_err(why).to_string();
        assert!(err.contains("refused"), "{why}: {err}");
        assert_eq!(self.snapshot(), before, "{why}: refusal changed state");
    }
}

#[test]
fn enrolls_over_s3_and_admits_through_s1_then_refuses_re_enroll() {
    let f = Fixture::new();
    let plan = plan_enrollment(&f.request()).expect("dry-run preflight");
    assert!(!plan.resume);
    assert_eq!(plan.authority.generation, 0);
    assert!(f.listing().is_empty(), "dry run wrote nothing remote");
    assert!(!f.inventory().exists() && !f.journal.exists() && !f.intent().exists());

    let receipt = enroll_owner(&f.request()).expect("enrolled");
    assert_eq!(receipt, plan.receipt, "receipt is the planned one");
    assert_eq!(f.listing(), vec!["active/00".to_string()]);
    assert!(!f.intent().exists(), "intent retired after verification");
    for path in [f.inventory(), f.journal.clone()] {
        assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o077, 0);
    }
    // A fresh process can admit it with only the inventory and the witness.
    let source = VerifySource {
        profile: PROFILE,
        witness: ObjectWitness::attach(receipt.store_id.clone(), &f.backend),
    };
    drop(
        Store::open_enrolled(
            f.db.to_str().unwrap(),
            &EnrolledStartup {
                inventory: &f.inventory(),
                witness: &source,
            },
        )
        .expect("real S1 admission"),
    );
    f.refuses(&f.request(), "re-enroll");
}

#[test]
fn crash_after_steps_three_four_five_resumes_to_the_identical_receipt() {
    for step in [3u8, 4, 5] {
        let f = Fixture::new();
        let planned = plan_enrollment(&f.request()).unwrap().receipt;
        let hooks = Hooks {
            crash_after: Some(step),
            ..Hooks::default()
        };
        let err = run(&f.request(), &hooks).expect_err("injected crash");
        assert!(err.to_string().contains("injected crash"), "{err}");
        assert!(
            f.intent().exists(),
            "step {step}: intent survives the crash"
        );
        let plan = plan_enrollment(&f.request()).expect("resumable preflight");
        assert!(plan.resume, "step {step}: resume detected");
        assert_eq!(enroll_owner(&f.request()).unwrap(), planned, "step {step}");
        assert!(!f.intent().exists());
        assert_eq!(f.listing(), vec!["active/00".to_string()]);
    }
}

#[test]
fn unknown_genesis_create_resumes_by_exact_readback() {
    let f = Fixture::new();
    f.emulator.push(EmuFault::LostReplyOnPut);
    assert!(enroll_owner(&f.request()).is_err(), "lost reply is UNKNOWN");
    assert_eq!(
        f.listing(),
        vec!["active/00".to_string()],
        "create committed"
    );
    assert!(enroll_owner(&f.request()).is_ok(), "exact bytes resume");
}

#[test]
fn refusals_leave_db_inventory_and_witness_unchanged() {
    // Profile mismatch.
    let f = Fixture::new();
    f.refuses(&f.request_with("other-profile"), "profile mismatch");

    // Existing marker.
    let f = Fixture::new();
    let conn = Connection::open(&f.db).unwrap();
    conn.execute_batch(MARKER_SQL).unwrap();
    conn.execute(
        "INSERT INTO ownership_handoff_enrollment(id, binding) VALUES (1, 'x')",
        [],
    )
    .unwrap();
    drop(conn);
    f.refuses(&f.request(), "existing marker");

    // Existing inventory.
    let f = Fixture::new();
    fs::write(f.inventory(), b"{}").unwrap();
    fs::set_permissions(f.inventory(), fs::Permissions::from_mode(0o600)).unwrap();
    f.refuses(&f.request(), "existing inventory");

    // Witness genesis with different bytes.
    let f = Fixture::new();
    let mut other = plan_enrollment(&f.request()).unwrap().authority;
    other.epoch = "a-different-epoch".into();
    let (key, bytes) = genesis_record(&other).unwrap();
    assert_eq!(f.backend.create(&key, &bytes), Reply::Ok(Created::New));
    f.refuses(&f.request(), "different genesis bytes");

    // Group-readable journal.
    let f = Fixture::new();
    let plan = plan_enrollment(&f.request()).unwrap();
    ownership_journal::create(
        &f.journal,
        &Journal::Active {
            authority: plan.authority,
        },
    )
    .unwrap();
    fs::set_permissions(&f.journal, fs::Permissions::from_mode(0o640)).unwrap();
    f.refuses(&f.request(), "group-readable journal");

    // Database path is a symlink.
    let f = Fixture::new();
    let link = f.dir.path().join("link.db");
    std::os::unix::fs::symlink(&f.db, &link).unwrap();
    let mut req = f.request();
    req.db_path = &link;
    f.refuses(&req, "symlinked database");

    // Witness unreachable.
    let f = Fixture::new();
    f.emulator.push(EmuFault::Status500);
    let before = (fs::read(&f.db).unwrap(), f.inventory().exists());
    assert!(enroll_owner(&f.request()).is_err(), "unreachable witness");
    assert_eq!((fs::read(&f.db).unwrap(), f.inventory().exists()), before);
}

#[test]
fn database_replaced_between_preflight_and_marker_is_refused() {
    let f = Fixture::new();
    let original = fs::read(&f.db).unwrap();
    let swap = || {
        let tmp = f.dir.path().join("swap.db");
        fs::copy(&f.db, &tmp).unwrap();
        fs::rename(&tmp, &f.db).unwrap();
    };
    let hooks = Hooks {
        before_marker: Some(&swap),
        ..Hooks::default()
    };
    let err = run(&f.request(), &hooks).expect_err("inode changed");
    assert!(err.to_string().contains("device/inode changed"), "{err}");
    assert_eq!(fs::read(&f.db).unwrap(), original, "no marker written");
}

#[test]
fn pilot_entry_point_refuses_group_readable_credentials_and_dry_runs() {
    let f = Fixture::new();
    let creds = f.dir.path().join("credentials.json");
    fs::write(
        &creds,
        br#"{"access_key_id":"AKIAIOSFODNN7EXAMPLE","secret_access_key":"wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY"}"#,
    )
    .unwrap();
    fs::set_permissions(&creds, fs::Permissions::from_mode(0o640)).unwrap();
    let config: crate::config::IngestPilotConfig = toml::from_str(&format!(
        r#"enabled = true
graph = "urn:g"
root = "/r"
inventory = "/r/i"
profile = "{PROFILE}"
endpoint = "{}"
bucket = "witness"
region = "us-east-1"
prefix = "lineage/abc"
timeout_secs = 1
installed_policy = "pilot-policy"
credentials_file = "{}"
catalog_file = "/r/c"
memory_mib = 64
"#,
        f.emulator.endpoint,
        creds.display()
    ))
    .unwrap();
    let req = super::super::PilotEnroll {
        db: &f.db,
        journal: &f.journal,
        unit: "full-store",
        principal: "pilot-owner",
        execute: false,
    };
    let err = super::super::enroll_pilot_owner(&config, &req).unwrap_err();
    assert!(err.to_string().contains("unsafe permissions"), "{err}");
    assert!(
        !err.to_string().contains("EXAMPLEKEY"),
        "no secret in errors"
    );
    fs::set_permissions(&creds, fs::Permissions::from_mode(0o600)).unwrap();
    let report = super::super::enroll_pilot_owner(&config, &req).unwrap();
    assert_eq!(report["mode"], "dry-run");
    assert!(f.listing().is_empty() && !f.journal.exists());
    let execute = super::super::PilotEnroll {
        execute: true,
        ..req.clone()
    };
    let report = super::super::enroll_pilot_owner(&config, &execute).unwrap();
    assert_eq!(report["mode"], "executed");
    assert_eq!(report["receipt"]["witness_key"], "active/00");
}
