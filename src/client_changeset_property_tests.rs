//! Generated round-trip and canonical-order invariants of the portable wire.
use super::*;
use proptest::prelude::*;

proptest! {
    #![proptest_config(ProptestConfig { cases: 64, failure_persistence: None, ..ProptestConfig::default() })]

    #[test]
    fn client_overlay_wire_roundtrip_preserves_signed_bytes(
        chars in proptest::collection::vec(any::<char>(), 0..32),
    ) {
        let value: String = chars.into_iter().collect();
        let triple = format!("<urn:s> <urn:p> {}", oxrdf::Literal::new_simple_literal(value));
        let mut local = overlay();
        let base = local.base().clone();
        local.replace([triple]).unwrap();
        let signed = local.sign(&key(), identity()).unwrap();
        let decoded: SignedChangeSet = serde_json::from_slice(&serde_json::to_vec(&signed).unwrap()).unwrap();
        decoded.verify(&quipu_signing::public_key_hex(&key())).unwrap();
        prop_assert_eq!(decoded.envelope.message().unwrap(), signed.envelope.message().unwrap());
        prop_assert_eq!(decoded.body, signed.body.clone());
        let restored: ClientOverlay = serde_json::from_slice(&serde_json::to_vec(&local).unwrap()).unwrap();
        prop_assert_eq!(restored.base(), &base);
        let again = restored.sign(&key(), identity()).unwrap();
        prop_assert_eq!(again.body, signed.body);
        prop_assert_eq!(again.signature, signed.signature);
    }

    #[test]
    fn client_base_hash_is_independent_of_row_order_and_duplicates(
        indices in proptest::collection::vec(0u16..100, 0..24),
    ) {
        let rows: Vec<_> = indices.into_iter().map(|index| BaseRow {
            triple: format!("<urn:row:{index}> <urn:p> \"{index}\""),
            valid_from: "2026-01-01T00:00:00Z".into(), valid_to: None,
        }).collect();
        let make = |rows| BaseJournal::new("urn:store:test".into(), "urn:epoch:test".into(),
            3, "ROOT".into(), digest(b"shapes"), rows).unwrap();
        let first = make(rows.clone());
        let mut reordered = rows.clone();
        reordered.reverse();
        reordered.extend(rows);
        let second = make(reordered);
        prop_assert_eq!(first.slice_hash().unwrap(), second.slice_hash().unwrap());
        prop_assert_eq!(first, second);
    }
}
