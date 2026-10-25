use super::*;
use proptest::prelude::*;

fn name(id: i64, score: f64) -> Candidate {
    Candidate::Name {
        entity_id: id,
        score,
        exact: false,
    }
}
fn fingerprint(candidates: Vec<EntityCandidate>) -> Vec<(String, u64, String)> {
    candidates
        .into_iter()
        .map(|c| (c.iri, c.score.to_bits(), c.matched_on))
        .collect()
}

#[test]
fn boundary_tier_exclusions_and_both_tie_contracts() {
    for (ties, expected) in [(Ties::Iri, "urn:a"), (Ties::Input, "urn:z")] {
        let mut reads = Vec::new();
        let result = select_with(
            vec![name(0, 1.0), name(1, 0.9), name(2, 0.9), name(3, 0.8)],
            &["urn:excluded".into()],
            1,
            ties,
            |id| {
                reads.push(id);
                Ok(
                    ["urn:excluded", "urn:z", "urn:a", "urn:lower"][usize::try_from(id).unwrap()]
                        .into(),
                )
            },
        )
        .unwrap();
        assert_eq!(result[0].iri, expected);
        assert_eq!(
            reads,
            [0, 1, 2],
            "resolve the whole boundary tier, never lower tiers"
        );
    }
}

#[test]
fn iri_dedup_does_not_assume_ids_are_one_to_one() {
    let mut reads = Vec::new();
    let result = select_with(
        vec![name(0, 1.0), name(0, 1.0), name(1, 1.0), name(2, 0.9)],
        &[],
        2,
        Ties::Input,
        |id| {
            reads.push(id);
            Ok(if id == 2 { "urn:second" } else { "urn:same" }.into())
        },
    )
    .unwrap();
    assert_eq!(result.len(), 2);
    assert_eq!(
        reads,
        [0, 1, 2],
        "cache repeated IDs but still dedup distinct IDs by resolved IRI"
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    #[test]
    fn lazy_selection_matches_full_legacy_pipeline(
        rows in prop::collection::vec((0u8..7, 0u8..11, any::<bool>()), 0..60),
        excluded in prop::collection::vec(0u8..7, 0..7),
        top_k in 0usize..10,
    ) {
        let iri = |id: u8| format!("urn:{}", 6-id);
        let excluded: Vec<_> = excluded.into_iter().map(iri).collect();
        for ties in [Ties::Iri, Ties::Input] {
            let mut eager: Vec<_> = rows.iter().map(|&(id, level, known)| {
                let score = f64::from(level) / 10.0;
                EntityCandidate { iri: iri(id), score,
                    matched_on: if known { "embedding:fixture".into() }
                    else { format!("canonical_name:jaro_winkler:{score:.2}") } }
            }).collect();
            eager.retain(|c| !excluded.contains(&c.iri));
            eager.sort_by(|a,b| {
                let order = b.score.partial_cmp(&a.score).unwrap();
                match ties { Ties::Iri => order.then_with(|| a.iri.cmp(&b.iri)), Ties::Input => order }
            });
            let mut seen = HashSet::new();
            eager.retain(|c| seen.insert(c.iri.clone()));
            eager.truncate(top_k);
            let deferred = rows.iter().map(|&(id, level, known)| {
                let score = f64::from(level) / 10.0;
                if known { Candidate::Known(EntityCandidate { iri: iri(id), score, matched_on: "embedding:fixture".into() }) }
                else { name(i64::from(id), score) }
            }).collect();
            let result = select_with(deferred, &excluded, top_k, ties, |id| Ok(iri(u8::try_from(id).unwrap()))).unwrap();
            prop_assert_eq!(fingerprint(result), fingerprint(eager));
        }
    }
}
