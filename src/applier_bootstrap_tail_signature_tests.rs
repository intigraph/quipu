//! Strict signed tail controls, including recomputed chunk digest attacks.
use super::*;

#[test]
fn tampered_tail_transaction_is_not_admitted_under_genuine_coverage() {
    for remove_signature in [false, true] {
        altered_tail_is_refused(remove_signature);
    }
}

fn altered_tail_is_refused(remove_signature: bool) {
    let (mut src, manifest, records, trust, signature, key) = signed_cut();
    let (mut replica, mut applier) = empty();
    applier.require_signed_bootstrap(trust.clone()).unwrap();
    applier
        .apply_frame(
            &mut replica,
            "quipu.asof",
            &signed_frame(&manifest, &records, &signature).to_string(),
            0,
        )
        .unwrap();
    let request = crate::replicate_source::StreamRequest {
        graphs: vec!["ROOT".into(), BOARD.into()],
        context_graphs: vec![],
        since: manifest["watermark_tx"].as_i64().unwrap(),
        expected_store_id: src.store_id().unwrap(),
    };
    let (mut session, opening) = crate::replicate_source::Session::open(
        &src,
        &request,
        "cd".repeat(16),
        std::sync::Arc::new(key),
    )
    .unwrap();
    applier
        .apply_frame(&mut replica, opening.event, &opening.data, 1)
        .unwrap();
    let change = d(
        &src,
        "urn:item:new",
        "urn:p:status",
        Value::Str("authentic".into()),
        Op::Assert,
        None,
    );
    src.transact(&[change], T, None, None).unwrap();
    let nonce = [9_u8; 16];
    applier.challenge(session.id(), nonce, 10).unwrap();
    let (frames, answered) = session.poll(&src, 100, Some(nonce)).unwrap();
    assert!(answered);
    let (mut control_store, mut control) = empty();
    control.require_signed_bootstrap(trust).unwrap();
    control
        .apply_frame(
            &mut control_store,
            "quipu.asof",
            &signed_frame(&manifest, &records, &signature).to_string(),
            0,
        )
        .unwrap();
    control
        .apply_frame(&mut control_store, opening.event, &opening.data, 1)
        .unwrap();
    control.challenge(session.id(), nonce, 10).unwrap();
    for frame in &frames {
        let outcome = control
            .apply_frame(&mut control_store, frame.event, &frame.data, 11)
            .unwrap();
        assert!(!matches!(
            outcome,
            Outcome::Refused { .. } | Outcome::Resume { .. }
        ));
    }
    assert!(control.freshness(13).bounded);
    assert!(control.progress().unwrap().source_tx > request.since);
    let tx = frames
        .iter()
        .find(|frame| frame.event == crate::replicate_wire::event::TX)
        .unwrap();
    let mut data: Json = serde_json::from_str(&tx.data).unwrap();
    assert_eq!(data["records"][0]["value"], json!("authentic"));
    if remove_signature {
        data.as_object_mut().unwrap().remove("signature");
    } else {
        data["records"][0]["value"] = json!("altered-in-transit");
    }
    let outcome = applier
        .apply_frame(&mut replica, tx.event, &data.to_string(), 11)
        .unwrap();
    assert!(
        matches!(outcome, Outcome::Refused { .. }),
        "altered transaction accepted: {outcome:?}"
    );
    for frame in frames
        .iter()
        .filter(|frame| frame.event == crate::replicate_wire::event::COVERAGE)
    {
        applier
            .apply_frame(&mut replica, frame.event, &frame.data, 12)
            .unwrap();
    }
    assert!(!applier.freshness(13).bounded);
    assert_eq!(applier.progress().unwrap().source_tx, request.since);
}

#[test]
fn recomputed_chunk_chain_cannot_replace_a_source_transaction_signature() {
    use crate::replicate_wire::event;
    use sha2::{Digest, Sha256};
    let (mut src, manifest, records, trust, signature, key) = signed_cut();
    let request = crate::replicate_source::StreamRequest {
        graphs: vec!["ROOT".into(), BOARD.into()],
        context_graphs: vec![],
        since: manifest["watermark_tx"].as_i64().unwrap(),
        expected_store_id: src.store_id().unwrap(),
    };
    let (session, opening) = crate::replicate_source::Session::open(
        &src,
        &request,
        "ef".repeat(16),
        std::sync::Arc::new(key),
    )
    .unwrap();
    let mut session = session.with_part_max_len(2_000);
    let change = d(
        &src,
        "urn:item:new",
        "urn:p:status",
        Value::Str("authentic".into()),
        Op::Assert,
        None,
    );
    src.transact(&[change], T, None, None).unwrap();
    let nonce = [10_u8; 16];
    let (frames, answered) = session.poll(&src, 100, Some(nonce)).unwrap();
    assert!(answered);
    assert!(frames.iter().any(|f| f.event == event::TX_PART));
    for altered in [false, true] {
        let (mut replica, mut applier) = empty();
        applier.require_signed_bootstrap(trust.clone()).unwrap();
        applier
            .apply_frame(
                &mut replica,
                "quipu.asof",
                &signed_frame(&manifest, &records, &signature).to_string(),
                0,
            )
            .unwrap();
        applier
            .apply_frame(&mut replica, opening.event, &opening.data, 1)
            .unwrap();
        applier.challenge(session.id(), nonce, 10).unwrap();
        let mut hasher = Sha256::new();
        let mut parts = Vec::new();
        for frame in frames.iter().filter(|f| f.event == event::TX_PART) {
            let mut body: Json = serde_json::from_str(&frame.data).unwrap();
            if altered {
                assert_eq!(body["records"][0]["value"], json!("authentic"));
                body["records"][0]["value"] = json!("altered-in-transit");
            }
            body["digest_before"] = json!(hex::encode(hasher.clone().finalize()));
            let text = body.to_string();
            hasher.update(text.as_bytes());
            parts.push(text);
        }
        let digest = hex::encode(hasher.finalize());
        let mut part_index = 0;
        for frame in &frames {
            let mut body: Json = serde_json::from_str(&frame.data).unwrap();
            let text = if frame.event == event::TX_PART {
                let text = parts[part_index].clone();
                part_index += 1;
                text
            } else {
                if matches!(frame.event, event::TX_BEGIN | event::TX_COMMIT) {
                    body["digest"] = json!(digest);
                }
                body.to_string()
            };
            let outcome = applier
                .apply_frame(&mut replica, frame.event, &text, 11)
                .unwrap();
            if frame.event == event::TX_COMMIT {
                assert_eq!(
                    matches!(outcome, Outcome::Refused { .. }),
                    altered,
                    "{outcome:?}"
                );
            }
        }
        assert_eq!(applier.freshness(13).bounded, !altered);
        assert_eq!(
            applier.progress().unwrap().source_tx == request.since,
            altered
        );
    }
}
