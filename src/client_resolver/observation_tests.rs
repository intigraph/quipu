//! Consumer controls use actual writer observations, never fabricated records.
use super::*;
use crate::resolver_contract::{ProspectiveRow, ProspectiveTime, SourceBinding};
use crate::{Datum, Op, Store, Value};
const TS: &str = "2026-01-01T00:00:00Z";
const SHAPES: &str = "@prefix sh:<http://www.w3.org/ns/shacl#>. <urn:S> a sh:NodeShape; sh:targetSubjectsOf <urn:p>; sh:property [sh:path <urn:p>; sh:maxCount 1].";
fn observed(
    shapes: bool,
    violates: bool,
) -> (
    SignedChangeSet,
    SignedResolverArtifact,
    Vec<crate::store::ValidatorObservation>,
) {
    observed_in_graph(shapes, violates, "ROOT")
}
fn observed_in_graph(
    shapes: bool,
    violates: bool,
    graph: &str,
) -> (
    SignedChangeSet,
    SignedResolverArtifact,
    Vec<crate::store::ValidatorObservation>,
) {
    let mut store = Store::open_in_memory().unwrap();
    // Both named graphs belong to this same store; equal RDF must not erase
    // their distinct identities when a candidate is re-signed below.
    let named_a = store.graph_create("urn:graph:a").unwrap();
    let named_b = store.graph_create("urn:graph:b").unwrap();
    let graph_id = match graph {
        "ROOT" => crate::schema::ROOT_GRAPH,
        "urn:graph:a" => named_a,
        "urn:graph:b" => named_b,
        _ => panic!("unknown fixture graph"),
    };
    if shapes {
        store.load_shapes("one", SHAPES, TS).unwrap();
    }
    // The offline v1 base requires an existing cursor even without shapes.
    let seed = Datum {
        entity: store.intern("urn:seed").unwrap(),
        attribute: store.intern("urn:seed:p").unwrap(),
        value: Value::Str("seed".into()),
        valid_from: TS.into(),
        valid_to: None,
        op: Op::Assert,
    };
    store.transact(&[seed], TS, None, None).unwrap();
    let journal = BaseJournal::new(
        store.store_id().unwrap(),
        store.log_epoch().unwrap(),
        store.latest_tx_id().unwrap(),
        graph.into(),
        digest(
            store
                .get_combined_shapes()
                .unwrap()
                .unwrap_or_default()
                .as_bytes(),
        ),
        [],
    )
    .unwrap();
    let mut overlay = ClientOverlay::open(journal, "urn:fork:actual".into()).unwrap();
    overlay.replace(["<urn:e> <urn:p> \"ok\"".into()]).unwrap();
    let proposal = overlay
        .sign(
            &key(7),
            ProposalIdentity {
                key_id: "urn:key:actual".into(),
                signer: "actual".into(),
                operation_id: "00000000-0000-4000-8000-000000000002".into(),
                issued_at: TS.into(),
                nonce: "22".repeat(16),
            },
        )
        .unwrap();
    let e = store.intern("urn:e").unwrap();
    let a = store.intern("urn:p").unwrap();
    let mut data = vec![Datum {
        entity: e,
        attribute: a,
        value: Value::Str("ok".into()),
        valid_from: TS.into(),
        valid_to: None,
        op: Op::Assert,
    }];
    if violates {
        let mut second = data[0].clone();
        second.value = Value::Str("second".into());
        data.push(second);
    }
    let (result, observations) =
        store.observe_validator(|s| s.transact_to_graph(&data, TS, None, None, graph_id));
    assert_eq!(result.is_err(), violates);
    assert_eq!(observations.len(), 1);
    let manifest = observations[0].manifest().unwrap().clone();
    let candidate = ProspectiveCandidate {
        source: SourceBinding {
            store_id: proposal.envelope.store_id.clone(),
            log_epoch: proposal.envelope.log_epoch.clone(),
            base_tx: u64::try_from(proposal.envelope.base_tx).unwrap(),
            graph: proposal.envelope.graph.clone(),
            base_slice_hash: digest_value(&proposal.envelope.base_slice_hash).unwrap(),
            scope: None,
            inputs: ["base", "ours", "theirs", "decisions"]
                .map(|name| crate::resolver_contract::NamedContent {
                    name: name.into(),
                    version: "fixture-v1".into(),
                    content: vec![],
                })
                .to_vec(),
        },
        rows: crate::share_merge::parse_graph(
            std::str::from_utf8(&manifest.staged_graph).unwrap(),
            "actual staged fixture",
        )
        .unwrap()
        .into_iter()
        .map(|triple| ProspectiveRow {
            triple: triple.to_string(),
            valid_from: ProspectiveTime::RetainedUtc(TS.into()),
            valid_to: None,
        })
        .collect(),
        decisions: vec![],
        validator: manifest.identity().unwrap(),
    };
    let signed = ResolverArtifact::new(&proposal, candidate, manifest)
        .unwrap()
        .sign(&proposal, &key(7))
        .unwrap();
    (proposal, signed, observations)
}
#[test]
fn actual_success_and_violation_keep_their_gate_outcomes() {
    for violates in [false, true] {
        let (proposal, signed, observations) = observed(true, violates);
        let evidence = signed
            .bind_observation(
                &proposal,
                &quipu_signing::public_key_hex(&key(7)),
                &signed.artifact.candidate,
                &observations,
            )
            .unwrap();
        println!(
            "actual writer: raw_conforms={:?}, gate_error={}, bound={:?}",
            observations[0].validator_conforms(),
            observations[0].gate_error().is_some(),
            evidence.outcome
        );
        assert_eq!(
            evidence.outcome,
            if violates {
                ValidationOutcome::Violates
            } else {
                ValidationOutcome::Conforms
            }
        );
    }
}
#[test]
fn absent_or_ambiguous_observations_refuse_and_no_shapes_remains_unknown() {
    let (proposal, signed, observations) = observed(false, false);
    let pk = quipu_signing::public_key_hex(&key(7));
    assert_eq!(
        signed
            .bind_observation(&proposal, &pk, &signed.artifact.candidate, &observations)
            .unwrap()
            .outcome,
        ValidationOutcome::Unknown
    );
    assert!(
        signed
            .bind_observation(&proposal, &pk, &signed.artifact.candidate, &[])
            .is_err()
    );
    println!("unexecuted validator: bound=Unknown; empty observation scope refused");
    let mut two = observations.clone();
    two.extend(observations);
    assert!(
        signed
            .bind_observation(&proposal, &pk, &signed.artifact.candidate, &two)
            .is_err()
    );
}
#[test]
fn resigned_supplied_manifest_and_reconstructed_candidate_cannot_replace_actual_evidence() {
    let (proposal, signed, observations) = observed(true, false);
    let pk = quipu_signing::public_key_hex(&key(7));
    let mut changed = signed.artifact.clone();
    changed.manifest.flags.recording_verdicts = !changed.manifest.flags.recording_verdicts;
    changed.candidate.validator = changed.manifest.identity().unwrap();
    let changed = changed.sign(&proposal, &key(7)).unwrap();
    assert!(
        changed
            .bind_observation(&proposal, &pk, &changed.artifact.candidate, &observations)
            .is_err()
    );
    println!("valid signature over substituted manifest: refused against actual observation");
    let mut candidate = signed.artifact.candidate.clone();
    candidate.rows.clear();
    assert!(
        signed
            .bind_observation(&proposal, &pk, &candidate, &observations)
            .is_err()
    );
    let mut foreign_proposal = proposal.clone();
    foreign_proposal.envelope.store_id.push_str("-other");
    foreign_proposal.body = foreign_proposal.body.replace(
        &proposal.envelope.store_id,
        &foreign_proposal.envelope.store_id,
    );
    foreign_proposal.envelope.body_sha256 = digest(foreign_proposal.body.as_bytes());
    foreign_proposal.signature =
        quipu_signing::sign_hex(&key(7), &foreign_proposal.envelope.message().unwrap());
    let mut foreign = signed.artifact.clone();
    foreign.proposal_fingerprint = foreign_proposal.fingerprint().unwrap();
    foreign.candidate.source.store_id = foreign_proposal.envelope.store_id.clone();
    let foreign = foreign.sign(&foreign_proposal, &key(7)).unwrap();
    assert!(
        foreign
            .bind_observation(
                &foreign_proposal,
                &pk,
                &foreign.artifact.candidate,
                &observations
            )
            .is_err()
    );
    // Even a freshly re-signed candidate and agreeing reconstruction cannot
    // bind the actual validator result to different staged RDF.
    let mut substituted = signed.artifact.clone();
    substituted.candidate.rows.clear();
    let substituted = substituted.sign(&proposal, &key(7)).unwrap();
    assert!(
        substituted
            .bind_observation(
                &proposal,
                &pk,
                &substituted.artifact.candidate,
                &observations
            )
            .is_err()
    );
    println!("re-signed unrelated candidate: refused against actual staged graph");
}

fn rebind_graph(
    mut proposal: SignedChangeSet,
    signed: SignedResolverArtifact,
    graph: &str,
) -> (SignedChangeSet, SignedResolverArtifact) {
    let mut changeset = proposal.changeset().unwrap();
    for row in changeset.deletes.iter_mut().chain(&mut changeset.inserts) {
        row.graph = (graph != "ROOT").then(|| graph.to_owned());
    }
    proposal.body = changeset.to_sparql_update();
    proposal.envelope.graph = graph.into();
    proposal.envelope.scope_digest = crate::asof_share::scope_digest(&[graph.into()], &[]);
    proposal.envelope.delta_sha256 = format!("sha256:{}", changeset.digest());
    proposal.envelope.body_sha256 = digest(proposal.body.as_bytes());
    proposal.signature = quipu_signing::sign_hex(&key(7), &proposal.envelope.message().unwrap());
    let pk = quipu_signing::public_key_hex(&key(7));
    proposal.verify(&pk).unwrap();
    let mut artifact = signed.artifact;
    artifact.proposal_fingerprint = proposal.fingerprint().unwrap();
    artifact.candidate.source.graph = graph.into();
    let signed = artifact.sign(&proposal, &key(7)).unwrap();
    signed.verify(&proposal, &pk).unwrap();
    (proposal, signed)
}

#[test]
fn actual_graph_identity_cannot_be_rebound_by_resigning_equal_rdf() {
    for (actual, supplied) in [
        ("ROOT", "urn:graph:a"),
        ("urn:graph:a", "ROOT"),
        ("urn:graph:a", "urn:graph:b"),
        ("urn:graph:b", "urn:graph:a"),
    ] {
        let (proposal, signed, observations) = observed_in_graph(true, false, actual);
        assert_eq!(observations[0].validator_conforms(), Some(true));
        let actual_id = observations[0].graph();
        assert_eq!(actual_id == crate::schema::ROOT_GRAPH, actual == "ROOT");
        let (proposal, signed) = rebind_graph(proposal, signed, supplied);
        let result = signed.bind_observation(
            &proposal,
            &quipu_signing::public_key_hex(&key(7)),
            &signed.artifact.candidate,
            &observations,
        );
        println!("actual graph={actual} id={actual_id}, signed={supplied}: {result:?}");
        assert!(
            result.is_err(),
            "actual {actual} must not certify {supplied}"
        );
    }
}

#[test]
fn named_graph_binding_refuses_until_actual_iri_evidence_is_available() {
    for graph in ["urn:graph:a", "urn:graph:b"] {
        let (proposal, signed, observations) = observed_in_graph(true, false, graph);
        assert_ne!(observations[0].graph(), crate::schema::ROOT_GRAPH);
        assert_eq!(observations[0].validator_conforms(), Some(true));
        assert!(
            signed
                .bind_observation(
                    &proposal,
                    &quipu_signing::public_key_hex(&key(7)),
                    &signed.artifact.candidate,
                    &observations,
                )
                .is_err()
        );
    }
}
