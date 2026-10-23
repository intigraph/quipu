//! The SHACL write-gate profile is observation only (aegis-wlf0).
use quipu::{Store, Value};

use super::{SHAPES, TS, batch, values};

#[test]
fn poststate_profile_times_gated_writes_without_changing_decisions() {
    // aegis-wlf0: the profile is observation only. It counts a run only when
    // the gate actually has shapes, resets on take, and the gate refuses the
    // same write it refused without it.
    let mut store = Store::open_in_memory().unwrap();
    store.load_shapes("max", SHAPES, TS).unwrap();
    // Explicit: shacl builds default validate_on_write to true.
    store.shacl_config_mut().validate_on_write = false;
    let a = batch(&store, "A", true);
    store.transact(&a, TS, None, None).unwrap();
    assert_eq!(
        store.take_poststate_profile().runs,
        0,
        "an ungated write reaches no shapes"
    );

    store.shacl_config_mut().validate_on_write = true;
    let b = batch(&store, "B", true);
    assert!(store.transact(&b, TS, None, None).is_err());
    let profile = store.take_poststate_profile();
    assert_eq!(profile.runs, 1, "the refused write was gated once");
    assert_eq!(
        store.take_poststate_profile(),
        quipu::store::PoststateProfile::default(),
        "take resets the profile"
    );
    assert_eq!(values(&store, 0), vec![Value::Str("A".into())]);
}
