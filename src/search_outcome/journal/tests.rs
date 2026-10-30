#![cfg(unix)]
use super::*;
use crate::search_outcome::observation::{Call, Surface};
use crate::search_outcome::{Operation, Outcome, Summary};
use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};

fn event() -> Event {
    Call::new(Surface::Library, Operation::Search)
        .finish(None, Some(Summary::unavailable(Outcome::Unknown)))
        .unwrap()
}

#[test]
fn restart_appends_new_private_journals_without_rewriting_old_evidence() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let first = Journal::new(Some(directory.path()));
    let original = event();
    first.append(&original);
    let old_path = std::fs::read_dir(directory.path())
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let old_bytes = std::fs::read(&old_path).unwrap();
    let old_modified = std::fs::metadata(&old_path).unwrap().modified().unwrap();
    drop(first);
    let second = Journal::new(Some(directory.path()));
    let next = event();
    assert_ne!(original.logical_call_id, next.logical_call_id);
    second.append(&next);
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 2);
    assert_eq!(std::fs::read(&old_path).unwrap(), old_bytes);
    assert_eq!(
        std::fs::metadata(&old_path).unwrap().modified().unwrap(),
        old_modified
    );
    for entry in std::fs::read_dir(directory.path()).unwrap() {
        let path = entry.unwrap().path();
        assert_eq!(std::fs::metadata(&path).unwrap().mode() & 0o777, 0o600);
        let bytes = std::fs::read(path).unwrap();
        assert_eq!(bytes.last(), Some(&b'\n'));
        let record: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(record["schema"], "quipu-search-outcome/v1");
        assert!(!record["logical_call_id"].as_str().unwrap().is_empty());
    }
}

#[test]
fn broad_permissions_and_symlink_directory_are_refused_without_mutation() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
    let journal = Journal::new(Some(directory.path()));
    journal.append(&event());
    assert_eq!(journal.written.load(Ordering::Relaxed), 0);
    assert_eq!(journal.failed.load(Ordering::Relaxed), 1);
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
    assert_eq!(
        std::fs::metadata(directory.path()).unwrap().mode() & 0o777,
        0o755
    );
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let parent = tempfile::tempdir().unwrap();
    let alias = parent.path().join("alias");
    symlink(directory.path(), &alias).unwrap();
    let journal = Journal::new(Some(&alias));
    journal.append(&event());
    assert_eq!(journal.failed.load(Ordering::Relaxed), 1);
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
}

#[test]
fn disabled_and_failed_capture_are_visible_without_claiming_durability() {
    let disabled = Journal::new(None);
    disabled.append(&event());
    let mut rendered = String::new();
    disabled.render(&mut rendered);
    assert!(rendered.contains("outcome=\"disabled\"} 1"));
    assert!(!rendered.contains("last_success_timestamp_seconds"));

    let fixture = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(fixture.path(), b"retained-evidence\n").unwrap();
    let failed = Journal::new(None);
    *failed.sink.lock().unwrap() = Sink::Ready(std::fs::File::open(fixture.path()).unwrap());
    failed.append(&event());
    failed.append(&event());
    assert_eq!(failed.failed.load(Ordering::Relaxed), 2);
    assert_eq!(failed.written.load(Ordering::Relaxed), 0);
    assert_eq!(
        std::fs::read(fixture.path()).unwrap(),
        b"retained-evidence\n"
    );
}
