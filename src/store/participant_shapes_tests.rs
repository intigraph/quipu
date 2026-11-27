//! aegis-n0wc.5: write shapes reach a graph-set file only through its owner,
//! and a fenced participant file refuses a shape-registry write on EVERY
//! path. Before this, `GraphSetOwner` had no shapes-load API, so member
//! shapes reached a set file only by a direct `Store` open, which skipped
//! the write fence: the registry write moved the head and the S1
//! expected-head guard then stranded the pending batch.
use super::*;

const SHAPES: &str = "@prefix sh: <http://www.w3.org/ns/shacl#> .
<urn:shape:item> a sh:NodeShape ; sh:targetNode <urn:item> .";
const AT: &str = "2026-11-27T12:00:00Z";

fn shapes(owner: &GraphSetOwner) -> Option<String> {
    owner.owner.read(Store::get_combined_shapes).unwrap()
}

fn plain(files: &Files, index: usize) -> Store {
    let path = files.root.join(format!("{}.db", FILES[index].0));
    Store::open(path.to_str().unwrap()).unwrap()
}

/// Leave alpha (0) and beta (1) holding a prepared intent.
fn fenced(files: &Files) {
    let (alpha, beta) = (open(files, 0), open(files, 1));
    let batch = ParticipantBatch {
        batch_id: "batch-1".into(),
        members: members(files, &[&alpha, &beta], &[0, 1], "v1"),
    };
    let error =
        as_writer(|| with_crash(CrashPoint::AfterPrepare(1), || commit(&batch))).unwrap_err();
    assert!(error.to_string().contains("injected"), "{error}");
}

#[test]
fn shapes_load_through_the_set_owner_and_read_back() {
    let temp = tempfile::tempdir().unwrap();
    let files = files(temp.path());
    let alpha = open(&files, 0);
    alpha.load_shapes("member", SHAPES, AT).unwrap();
    assert!(
        shapes(&alpha).is_some_and(|s| s.contains("urn:shape:item")),
        "the loaded shapes are not visible through the owner"
    );
}

#[test]
fn a_fenced_set_owner_refuses_a_shapes_load_and_changes_nothing() {
    let temp = tempfile::tempdir().unwrap();
    let files = files(temp.path());
    fenced(&files);
    let head = raw_head(&files, 0);
    let alpha = open(&files, 0);
    let error = alpha.load_shapes("member", SHAPES, AT).unwrap_err();
    assert!(error.to_string().contains("participant fenced"), "{error}");
    assert_eq!(raw_head(&files, 0), head, "a refused load moved the head");
}

/// dearing's condition: the bypass must not survive beside the new API. A
/// plain `Store` open of a fenced member file can neither load nor remove
/// shapes, so no caller of `Store::load_shapes` can strand the batch.
#[test]
fn a_plain_open_of_a_fenced_member_file_cannot_change_its_shapes() {
    let temp = tempfile::tempdir().unwrap();
    let files = files(temp.path());
    {
        let alpha = open(&files, 0);
        alpha.load_shapes("member", SHAPES, AT).unwrap();
    }
    fenced(&files);
    let head = raw_head(&files, 0);
    let mut store = plain(&files, 0);
    let load = store.load_shapes("other", SHAPES, AT).unwrap_err();
    assert!(load.to_string().contains("participant fenced"), "{load}");
    let remove = store.remove_shapes("member").unwrap_err();
    assert!(
        remove.to_string().contains("participant fenced"),
        "{remove}"
    );
    assert_eq!(
        raw_head(&files, 0),
        head,
        "a refused registry write moved the head"
    );
}

/// An ordinary store with no participant tables is unaffected.
#[test]
fn an_ordinary_store_still_loads_shapes() {
    let mut store = Store::open_in_memory().unwrap();
    store.load_shapes("plain", SHAPES, AT).unwrap();
    assert!(store.get_combined_shapes().unwrap().is_some());
}
