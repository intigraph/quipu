//! Real verified TLS to the conditional-object emulator; no verifier bypass.
use super::*;
use crate::ownership_handoff_model::witness::objects::{Created, Objects};
use crate::ownership_handoff_model::witness::s3_emulator::{BUCKET, Emulator, PREFIX};
use crate::ownership_handoff_model::witness::s3_tls_bridge::TlsBridge;
use crate::ownership_handoff_model::witness::{Reply, assumed_genesis_bytes};
use crate::ownership_handoff_model::{Authority, AuthorityUnit, Journal, Placement};
use crate::store::{ownership_admission, ownership_inventory, ownership_journal};
use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};

fn enroll(f: &Fixture, c: &EnrolledPilotConfig, emulator: &Emulator) {
    let authority = Authority {
        store_id: f.route.store_id.clone(),
        unit: AuthorityUnit::FullStore,
        generation: 0,
        epoch: f.route.log_epoch.clone(),
        owner: Placement {
            principal: "fixture-owner".into(),
            native: [1; 32],
        },
        policy: f.route.policy.clone(),
        predecessor: None,
    };
    let journal = f.root.join("owner.journal");
    ownership_journal::create(
        &journal,
        &Journal::Active {
            authority: authority.clone(),
        },
    )
    .unwrap();
    fs::set_permissions(&journal, fs::Permissions::from_mode(0o600)).unwrap();
    let db = f.root.join("ingest.db");
    let meta = fs::metadata(&db).unwrap();
    let entry = ownership_inventory::Entry {
        store_id: f.route.store_id.clone(),
        unit: AuthorityUnit::FullStore,
        database: ownership_inventory::Database {
            path: db.to_str().unwrap().into(),
            device: meta.dev(),
            inode: meta.ino(),
        },
        journal: journal.to_str().unwrap().into(),
        witness: c.profile.clone(),
        enrolled_at_generation: 0,
    };
    let binding = ownership_inventory::Binding {
        version: 1,
        store_id: f.route.store_id.clone(),
        inventory_digest: entry.digest().unwrap(),
    };
    fs::write(
        &c.inventory,
        ownership_inventory::Inventory {
            version: 1,
            records: vec![entry],
        }
        .encode()
        .unwrap(),
    )
    .unwrap();
    fs::set_permissions(&c.inventory, fs::Permissions::from_mode(0o600)).unwrap();
    let conn = rusqlite::Connection::open(db).unwrap();
    conn.execute_batch(ownership_admission::MARKER_SQL).unwrap();
    conn.execute(
        "INSERT INTO ownership_handoff_enrollment(id,binding) VALUES(1,?1)",
        [serde_json::to_string(&binding).unwrap()],
    )
    .unwrap();
    assert_eq!(
        emulator
            .backend()
            .create("active/00", &assumed_genesis_bytes(&authority)),
        Reply::Ok(Created::New)
    );
}

#[test]
fn enrolled_factory_admits_over_verified_tls_and_refuses_untrusted_tls() {
    let f = fixture();
    f.legacy.fence_routed_graph(GRAPH, 7, 4).unwrap();
    let emulator = Emulator::start();
    let tls = TlsBridge::start(&emulator.endpoint);
    let mut c = config(&f);
    c.endpoint = tls.endpoint.clone();
    c.bucket = BUCKET.into();
    c.region = "us-east-1".into();
    c.prefix = PREFIX.into();
    enroll(&f, &c, &emulator);
    let credentials = || {
        Ok((
            "AKIAIOSFODNN7EXAMPLE".into(),
            "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY".into(),
        ))
    };
    assert!(
        enabled()
            .admit_enrolled(
                catalog_for(&f.route),
                &f.legacy,
                &c,
                credentials,
                |_| Ok(())
            )
            .is_err()
    );
    c.witness_ca_der = Some(tls.der.clone());
    let router = enabled()
        .admit_enrolled(
            catalog_for(&f.route),
            &f.legacy,
            &c,
            credentials,
            |_| Ok(()),
        )
        .expect("verified TLS S1 factory")
        .unwrap();
    assert_eq!(router.single_routed_graph(), Some(GRAPH));
    let mut request = request(Some(GRAPH));
    request.catalog_revision = router.catalog_revision();
    assert!(
        router
            .routed_knot(&request, &input(Some(GRAPH), "tls"))
            .unwrap()
            .is_some()
    );
    assert_eq!(
        f.legacy
            .conn
            .query_row("SELECT COUNT(*) FROM facts", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        0
    );
}

#[path = "graph_set_enrolled_tls_tests.rs"]
mod graph_set;

#[path = "graph_set_startup_tls_tests.rs"]
mod graph_set_startup;
