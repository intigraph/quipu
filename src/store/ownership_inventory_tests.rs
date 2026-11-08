use super::*;

fn fixture() -> (Inventory, Binding, Database) {
    let database = Database {
        path: "/example/data/store.db".into(),
        device: 7,
        inode: 42,
    };
    let entry = Entry {
        store_id: "urn:example:lineage".into(),
        unit: AuthorityUnit::FullStore,
        database: database.clone(),
        journal: "/protected/journal".into(),
        witness: "existing-profile-v1".into(),
        enrolled_at_generation: 3,
    };
    let binding = Binding {
        version: VERSION,
        store_id: entry.store_id.clone(),
        inventory_digest: entry.digest().unwrap(),
    };
    (
        Inventory {
            version: VERSION,
            records: vec![entry],
        },
        binding,
        database,
    )
}

#[test]
fn decision_table_and_binding_mismatches() {
    let (inventory, binding, database) = fixture();
    assert_eq!(decide(Marker::Absent, None, &database), Decision::Legacy);
    let empty = Inventory {
        version: VERSION,
        records: vec![],
    };
    assert_eq!(
        decide(Marker::Absent, Some(&empty), &database),
        Decision::Legacy
    );
    assert_eq!(
        decide(Marker::Known(&binding), Some(&inventory), &database),
        Decision::NeedsJournalAndWitness
    );
    assert_eq!(
        decide(Marker::Known(&binding), None, &database),
        Decision::Refuse
    );
    assert_eq!(
        decide(Marker::Known(&binding), Some(&empty), &database),
        Decision::Refuse
    );
    assert_eq!(
        decide(Marker::Absent, Some(&inventory), &database),
        Decision::Refuse
    );
    assert_eq!(
        decide(Marker::Unknown, Some(&inventory), &database),
        Decision::Refuse
    );
    let mut changed = binding.clone();
    changed.store_id.push('x');
    assert_eq!(
        decide(Marker::Known(&changed), Some(&inventory), &database),
        Decision::Refuse
    );
    let mut changed = binding.clone();
    changed.inventory_digest = "0".repeat(64);
    assert_eq!(
        decide(Marker::Known(&changed), Some(&inventory), &database),
        Decision::Refuse
    );
    let mut changed_inventory = inventory.clone();
    changed_inventory.records[0].enrolled_at_generation += 1;
    assert_eq!(
        decide(Marker::Known(&binding), Some(&changed_inventory), &database),
        Decision::Refuse
    );
    changed_inventory = inventory.clone();
    changed_inventory.records[0].witness.push('x');
    assert_eq!(
        decide(Marker::Known(&binding), Some(&changed_inventory), &database),
        Decision::Refuse
    );
    for (path, device, inode) in [
        ("/example/data/renamed.db", 7, 42), // rename or hard link
        ("/example/data/store.db", 7, 43),   // replaced file
        ("/example/data/copied.db", 7, 43),  // copied marker
        ("/example/data/store.db", 8, 42),   // another device
    ] {
        let actual = Database {
            path: path.into(),
            device,
            inode,
        };
        assert_eq!(
            decide(Marker::Known(&binding), Some(&inventory), &actual),
            Decision::Refuse
        );
    }
    // A markerless replacement is refused by either path or inode evidence.
    let mut replacement = database.clone();
    replacement.inode += 1;
    assert_eq!(
        decide(Marker::Absent, Some(&inventory), &replacement),
        Decision::Refuse
    );
    replacement = database.clone();
    replacement.path.push_str(".link");
    assert_eq!(
        decide(Marker::Absent, Some(&inventory), &replacement),
        Decision::Refuse
    );
}

#[test]
fn strict_codec_rejects_corruption_versions_fields_duplicates_and_bounds() {
    let (inventory, binding, _) = fixture();
    let bytes = inventory.encode().unwrap();
    assert_eq!(Inventory::decode(&bytes).unwrap(), inventory);
    assert_eq!(
        Binding::decode(&serde_json::to_vec(&binding).unwrap()).unwrap(),
        binding
    );
    for bytes in [
        vec![],
        vec![0xff],
        vec![b'x'; MAX_BYTES + 1],
        bytes[..bytes.len() - 1].to_vec(),
    ] {
        assert!(Inventory::decode(&bytes).is_err());
    }
    let mut corrupt = inventory.encode().unwrap();
    corrupt[0] ^= 1;
    assert!(Inventory::decode(&corrupt).is_err());
    let mut value = serde_json::to_value(&inventory).unwrap();
    value["extra"] = true.into();
    let body = serde_json::to_vec(&value).unwrap();
    let mut extra = body.clone();
    extra.extend_from_slice(TRAILER.as_bytes());
    extra.extend_from_slice(hex::encode(Sha256::digest(body)).as_bytes());
    extra.push(b'\n');
    assert!(Inventory::decode(&extra).is_err());
    let mut changed = inventory.clone();
    changed.version += 1;
    assert!(changed.encode().is_err());
    changed = inventory.clone();
    changed.records.push(changed.records[0].clone());
    assert!(changed.encode().is_err());
    changed = inventory.clone();
    changed.records[0].witness = "https://example.invalid".into();
    assert!(changed.encode().is_err());
    changed = inventory.clone();
    changed.records[0].database.path = "/data/../store".into();
    assert!(changed.encode().is_err());
    let duplicate = br#"{"version":1,"version":1,"store_id":"x","inventory_digest":"0"}"#;
    assert!(Binding::decode(duplicate).is_err());
    assert!(Binding::decode(&vec![b'x'; 2049]).is_err());
}

#[test]
fn configured_reader_and_real_file_aliases() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("inventory");
    let (inventory, _, _) = fixture();
    assert!(read(&path).is_err());
    assert!(read(Path::new("relative-inventory")).is_err());
    std::fs::write(&path, inventory.encode().unwrap()).unwrap();
    assert_eq!(read(&path).unwrap(), inventory);
    std::fs::write(&path, vec![b'x'; MAX_BYTES + 1]).unwrap();
    assert!(read(&path).is_err());
}

#[test]
fn every_identity_collision_is_rejected_and_unrelated_inventory_is_legacy() {
    let (inventory, _, database) = fixture();
    let mut unrelated = inventory.clone();
    unrelated.records[0].database.path.push_str(".other");
    unrelated.records[0].database.inode += 1;
    assert_eq!(
        decide(Marker::Absent, Some(&unrelated), &database),
        Decision::Legacy
    );
    for collision in 0..4 {
        let mut changed = inventory.clone();
        let mut other = changed.records[0].clone();
        other.store_id.push('2');
        other.database.path.push('2');
        other.database.inode += 1;
        other.journal.push('2');
        match collision {
            0 => other.store_id = changed.records[0].store_id.clone(),
            1 => other.database.path = changed.records[0].database.path.clone(),
            2 => other.database.inode = changed.records[0].database.inode,
            _ => other.journal = changed.records[0].journal.clone(),
        }
        changed.records.push(other);
        assert!(changed.encode().is_err(), "collision {collision}");
        assert_eq!(
            decide(Marker::Absent, Some(&changed), &database),
            Decision::Refuse
        );
    }
}

#[cfg(unix)]
#[test]
fn actual_copy_rename_and_hardlink_never_inherit_binding() {
    use std::os::unix::fs::MetadataExt;
    let dir = tempfile::tempdir().unwrap();
    let original = dir.path().join("original.db");
    std::fs::write(&original, b"fixture").unwrap();
    let identity = |path: &Path| {
        let meta = std::fs::metadata(path).unwrap();
        Database {
            path: std::fs::canonicalize(path)
                .unwrap()
                .to_str()
                .unwrap()
                .into(),
            device: meta.dev(),
            inode: meta.ino(),
        }
    };
    let (mut inventory, mut binding, _) = fixture();
    inventory.records[0].database = identity(&original);
    binding.inventory_digest = inventory.records[0].digest().unwrap();
    assert_eq!(
        decide(
            Marker::Known(&binding),
            Some(&inventory),
            &identity(&original)
        ),
        Decision::NeedsJournalAndWitness
    );
    let link = dir.path().join("linked.db");
    std::fs::hard_link(&original, &link).unwrap();
    let copy = dir.path().join("copy.db");
    std::fs::copy(&original, &copy).unwrap();
    let renamed = dir.path().join("renamed.db");
    std::fs::rename(&original, &renamed).unwrap();
    for path in [&link, &copy, &renamed] {
        assert_eq!(
            decide(Marker::Known(&binding), Some(&inventory), &identity(path)),
            Decision::Refuse
        );
    }
    std::fs::write(&original, b"replacement").unwrap();
    assert_eq!(
        decide(Marker::Absent, Some(&inventory), &identity(&original)),
        Decision::Refuse
    );
}
