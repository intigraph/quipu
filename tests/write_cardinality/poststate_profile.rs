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
    // Refusal must retain the incremental subphases, not just the outer total.
    assert!(profile.check_subjects > 0, "{profile:?}");
    assert!(profile.check_facts > 0, "{profile:?}");
    assert!(
        profile.check_validate > std::time::Duration::ZERO,
        "{profile:?}"
    );
    assert_eq!(profile.check_new_graph, std::time::Duration::ZERO);
    let phases = profile.check_graph
        + profile.check_ownership
        + profile.check_entities
        + profile.check_values
        + profile.check_encode
        + profile.check_validate
        + profile.check_debt;
    assert!(phases <= profile.check, "nested phases: {profile:?}");
    let entity_leaves = profile.check_entity_bounded
        + profile.check_entity_object_targets
        + profile.check_entity_reference_types
        + profile.check_entity_subclasses;
    assert!(
        entity_leaves <= profile.check_entities,
        "entity leaves: {profile:?}"
    );
    assert!(
        profile.check_entity_bounded > std::time::Duration::ZERO,
        "{profile:?}"
    );
    assert_eq!(
        store.take_poststate_profile(),
        quipu::store::PoststateProfile::default(),
        "take resets the profile"
    );
    assert_eq!(values(&store, 0), vec![Value::Str("A".into())]);
}

#[test]
fn observed_profile_excludes_an_earlier_unobserved_write() {
    // aegis-wlf0 (#756 review): /knot and /episode write through the shared
    // store without draining the profile, so an observed /set or /update must
    // report its own gate runs, not theirs.
    let mut store = Store::open_in_memory().unwrap();
    store.load_shapes("max", SHAPES, TS).unwrap();
    store.shacl_config_mut().validate_on_write = true;
    let unobserved = batch(&store, "A", true);
    store.transact(&unobserved, TS, None, None).unwrap();
    let observed = batch(&store, "A", true);
    let (result, profile) = store.observe_poststate(|s| s.transact(&observed, TS, None, None));
    result.unwrap();
    assert_eq!(profile.runs, 1, "only the observed write's gate run");
    assert_eq!(
        store.take_poststate_profile(),
        quipu::store::PoststateProfile::default(),
        "the observation drained the profile"
    );
}

#[test]
fn new_graph_check_is_timed() {
    // aegis-wlf0 (#756 review): a brand-new graph is judged whole in batch on
    // an early-return path, which must still record its check time.
    let mut store = Store::open_in_memory().unwrap();
    store.load_shapes("max", SHAPES, TS).unwrap();
    store.shacl_config_mut().validate_on_write = true;
    let first = batch(&store, "A", true);
    let (result, profile) = store.observe_poststate(|s| s.transact(&first, TS, None, None));
    result.unwrap();
    assert_eq!(profile.runs, 1);
    assert!(profile.check > std::time::Duration::ZERO, "{profile:?}");
    assert!(
        profile.check_new_graph > std::time::Duration::ZERO,
        "{profile:?}"
    );
    assert!(profile.check_graph + profile.check_new_graph <= profile.check);
    assert_eq!(profile.check_subjects, 0, "new graph uses its batch lane");
    assert_eq!(profile.check_validate, std::time::Duration::ZERO);
    assert_eq!(values(&store, 0), vec![Value::Str("A".into())]);
}
