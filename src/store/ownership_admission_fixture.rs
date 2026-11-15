//! Disposable enrollment for commit-boundary controls. No test pin setter:
//! every fixture traverses the same admission and installation as S1 startup.
use std::path::Path;

use super::Store;
use super::ownership_admission::{EnrolledStartup, MARKER_SQL, WitnessSource};
use super::ownership_inventory::{Binding, Database, Entry, Inventory};
use crate::ownership_handoff_model::witness::{InMemoryWitness, Reply};
use crate::ownership_handoff_model::{Authority, WitnessHead};

const PROFILE: &str = "private-admission-fixture";

struct Source(InMemoryWitness);

impl WitnessSource for Source {
    fn head_evidence(
        &self,
        profile: &str,
        store_id: &str,
        anchor: u64,
        _nonce: &[u8; 32],
    ) -> Reply<(WitnessHead, Vec<(String, String)>)> {
        if profile != PROFILE {
            return Reply::Unknown;
        }
        self.0.head_evidence(store_id, anchor)
    }
}

#[cfg(unix)]
pub(super) fn open(path: &Path, journal: &Path, authority: &Authority) -> Store {
    use std::fs;
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    let path = path.canonicalize().unwrap();
    let journal = journal.canonicalize().unwrap();
    fs::set_permissions(&journal, fs::Permissions::from_mode(0o600)).unwrap();
    let meta = fs::metadata(&path).unwrap();
    let entry = Entry {
        store_id: authority.store_id.clone(),
        unit: authority.unit.clone(),
        database: Database {
            path: path.to_str().unwrap().into(),
            device: meta.dev(),
            inode: meta.ino(),
        },
        journal: journal.to_str().unwrap().into(),
        witness: PROFILE.into(),
        enrolled_at_generation: authority.generation,
    };
    let binding = Binding {
        version: 1,
        store_id: authority.store_id.clone(),
        inventory_digest: entry.digest().unwrap(),
    };
    let inventory = journal.with_extension("inventory");
    fs::write(
        &inventory,
        Inventory {
            version: 1,
            records: vec![entry],
        }
        .encode()
        .unwrap(),
    )
    .unwrap();
    fs::set_permissions(&inventory, fs::Permissions::from_mode(0o600)).unwrap();
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch(MARKER_SQL).unwrap();
    conn.execute(
        "INSERT INTO ownership_handoff_enrollment(id, binding) VALUES (1, ?1)",
        [serde_json::to_string(&binding).unwrap()],
    )
    .unwrap();
    drop(conn);
    let Reply::Ok(witness) = InMemoryWitness::new(authority.clone()) else {
        panic!("fixture genesis");
    };
    Store::open_enrolled(
        path.to_str().unwrap(),
        &EnrolledStartup {
            inventory: &inventory,
            witness: &Source(witness),
        },
    )
    .expect("real S1 admission")
}

#[cfg(not(unix))]
pub(super) fn open(_: &Path, _: &Path, _: &Authority) -> Store {
    panic!("protected enrolled fixtures require Unix");
}
