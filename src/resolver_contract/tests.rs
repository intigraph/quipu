use super::*;
use crate::Error;

#[derive(serde::Deserialize)]
struct Vector {
    name: String,
    manifest: ValidatorManifest,
    candidate: ProspectiveCandidate,
    manifest_sha256: String,
    candidate_sha256: String,
    manifest_length: usize,
    candidate_length: usize,
    outcome: ValidationOutcome,
    manifest_hex: Option<String>,
    candidate_hex: Option<String>,
}

#[derive(serde::Deserialize)]
struct AdmissionVector {
    source_vector: String,
    actual: AdmissionCoordinates,
    candidate_sha256: String,
    binding_sha256: String,
}

#[derive(serde::Deserialize)]
struct Suite {
    contract: String,
    vectors: Vec<Vector>,
    admission: AdmissionVector,
}

fn suite() -> Suite {
    serde_json::from_str(include_str!(
        "../../test-fixtures/resolver-contract-v1.json"
    ))
    .unwrap()
}

fn base() -> (ValidatorManifest, ProspectiveCandidate) {
    let vector = suite().vectors.remove(0);
    (vector.manifest, vector.candidate)
}

#[test]
fn independent_joint_vectors_match_exact_bytes_hashes_and_outcomes() {
    let suite = suite();
    assert_eq!(suite.contract, "quipu-resolver-contract-v1");
    assert_eq!(suite.vectors.len(), 25);
    let mut distinct = BTreeSet::new();
    for vector in suite.vectors {
        let manifest = vector.manifest.canonical_bytes().unwrap();
        let candidate = vector.candidate.canonical_bytes().unwrap();
        assert_eq!(manifest.len(), vector.manifest_length, "{}", vector.name);
        assert_eq!(candidate.len(), vector.candidate_length, "{}", vector.name);
        assert_eq!(
            hex::encode(encoding::digest(&manifest).0),
            vector.manifest_sha256,
            "{}",
            vector.name
        );
        assert_eq!(
            hex::encode(encoding::digest(&candidate).0),
            vector.candidate_sha256,
            "{}",
            vector.name
        );
        if let Some(wire) = vector.manifest_hex {
            assert_eq!(hex::encode(&manifest), wire);
        }
        if let Some(wire) = vector.candidate_hex {
            assert_eq!(hex::encode(&candidate), wire);
        }
        assert!(distinct.insert(vector.candidate_sha256));
        let proof = vector
            .candidate
            .bind_validation(&vector.manifest, ValidationOutcome::Conforms)
            .unwrap();
        assert_eq!(proof.outcome, vector.outcome, "{}", vector.name);
    }
}

#[test]
fn manifest_mismatch_and_wrong_registry_join_refuse() {
    let (mut manifest, candidate) = base();
    manifest.flags.recording_verdicts = true;
    assert!(
        candidate
            .bind_validation(&manifest, ValidationOutcome::Conforms)
            .is_err()
    );
    let (mut manifest, _) = base();
    manifest.combined_shapes = manifest
        .loaded_shapes
        .iter()
        .flat_map(|v| &v.content)
        .copied()
        .collect();
    assert!(manifest.identity().is_err());
    let (mut manifest, _) = base();
    manifest.loaded_shapes.reverse();
    assert!(manifest.identity().is_err());
    let (mut manifest, _) = base();
    manifest.incremental.selected_shapes[0].version.push('x');
    assert!(manifest.identity().is_err());
}

#[test]
fn empty_data_interval_is_not_a_received_receipt() {
    let (_, mut candidate) = base();
    candidate.rows[0].valid_to = Some(candidate.rows[0].valid_from.clone());
    assert!(candidate.identity().is_err());
    candidate.rows[0].valid_to = None;
    candidate.decisions[0].operations = vec![ProspectiveOperation::Received {
        triple: candidate.rows[0].triple.clone(),
        at: ProspectiveTime::AdmissionAssigned,
    }];
    let (manifest, _) = base();
    assert_eq!(
        candidate
            .bind_validation(&manifest, ValidationOutcome::Conforms)
            .unwrap()
            .outcome,
        ValidationOutcome::RequiresAdmissionTime
    );
}

#[test]
fn canonical_terms_times_and_source_bindings_are_required() {
    let (_, mut candidate) = base();
    candidate.rows[0].valid_from = ProspectiveTime::RetainedUtc("2026-01-01T01:00:00+01:00".into());
    assert!(candidate.identity().is_err());
    let (_, mut candidate) = base();
    candidate.rows[0].triple = "_:unbound <urn:example:p> <urn:example:o>".into();
    assert!(candidate.identity().is_err());
    let (_, mut candidate) = base();
    candidate.source.inputs.pop();
    assert!(candidate.identity().is_err());
    let (_, mut candidate) = base();
    candidate.source.inputs[1] = candidate.source.inputs[0].clone();
    assert!(candidate.identity().is_err());
}

#[test]
fn no_implicit_validator_options_or_unknown_dto_fields() {
    let (mut manifest, candidate) = base();
    manifest.implementation.options.remove("engine");
    assert!(manifest.identity().is_err());
    let mut value = serde_json::to_value(candidate).unwrap();
    value["guessed_profile"] = serde_json::json!("full");
    assert!(serde_json::from_value::<ProspectiveCandidate>(value).is_err());
    let op = serde_json::json!({"close": {"triple":"x", "at":"admission_assigned", "extra":1}});
    assert!(serde_json::from_value::<ProspectiveOperation>(op).is_err());
}

#[test]
fn actual_admission_time_and_transaction_are_bound_after_fresh_validation() {
    let suite = suite();
    let expected = suite.admission;
    let vector = suite
        .vectors
        .into_iter()
        .find(|v| v.name == expected.source_vector)
        .unwrap();
    assert!(
        vector
            .candidate
            .bind_admission_validation(
                &vector.manifest,
                ValidationOutcome::Conforms,
                expected.actual.clone()
            )
            .is_err()
    );
    let assigned = vector
        .candidate
        .assign_admission_time(&expected.actual.accepted_at)
        .unwrap();
    assert_eq!(
        hex::encode(assigned.identity().unwrap().0),
        expected.candidate_sha256
    );
    let bound = assigned
        .bind_admission_validation(
            &vector.manifest,
            ValidationOutcome::Conforms,
            expected.actual.clone(),
        )
        .unwrap();
    assert_eq!(
        hex::encode(encoding::digest(&bound.canonical_bytes().unwrap()).0),
        expected.binding_sha256
    );
    let mut different_tx = bound.clone();
    different_tx.master.transaction += 1;
    assert_ne!(
        bound.canonical_bytes().unwrap(),
        different_tx.canonical_bytes().unwrap()
    );
    let mut different_time = expected.actual.clone();
    different_time.accepted_at = "2026-01-05T00:00:00Z".into();
    assert!(
        assigned
            .bind_admission_validation(
                &vector.manifest,
                ValidationOutcome::Conforms,
                different_time
            )
            .is_err()
    );
    let mut old_tx = expected.actual;
    old_tx.transaction = assigned.source.base_tx;
    assert!(
        assigned
            .bind_admission_validation(&vector.manifest, ValidationOutcome::Conforms, old_tx)
            .is_err()
    );
    assert!(
        assigned
            .bind_admission_validation(
                &vector.manifest,
                ValidationOutcome::Unknown,
                AdmissionCoordinates {
                    accepted_at: "2026-01-04T00:00:00Z".into(),
                    transaction: 23
                }
            )
            .is_err()
    );
}

#[test]
fn assignment_rechecks_intervals_without_changing_retained_times() {
    let (_, mut candidate) = base();
    let retained = candidate.rows[1].valid_from.clone();
    candidate.rows[0].valid_from = ProspectiveTime::AdmissionAssigned;
    candidate.rows[0].valid_to = Some(ProspectiveTime::RetainedUtc("2026-01-04T00:00:00Z".into()));
    assert!(
        candidate
            .assign_admission_time("2026-01-04T00:00:00Z")
            .is_err()
    );
    candidate.rows[0].valid_to = None;
    let assigned = candidate
        .assign_admission_time("2026-01-04T00:00:00Z")
        .unwrap();
    assert_eq!(assigned.rows[1].valid_from, retained);
    assert!(matches!(
        &assigned.rows[0].valid_from,
        ProspectiveTime::AdmissionResolved(_)
    ));
    assert!(matches!(
        candidate.assign_admission_time("not-time"),
        Err(Error::InvalidValue(_))
    ));
}
