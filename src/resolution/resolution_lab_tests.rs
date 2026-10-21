//! Explicit offline label parity. Never opens a graph, model, or endpoint.
use super::*;
use std::{path::PathBuf, sync::Arc, time::Instant};

fn old_candidates(index: &LabelIndex, name: &str) -> Vec<(i64, u64, bool)> {
    let lower = name.to_lowercase();
    let profile = LabelProfile::of(&lower);
    let commit = is_slash_qualified_commit_id(&lower);
    index
        .entries
        .iter()
        .filter_map(|entry| {
            if lower == entry.lower {
                return Some((entry.entity_id, 1.0f64.to_bits(), true));
            }
            if commit && is_slash_qualified_commit_id(&entry.lower) {
                return None;
            }
            if profile.jaro_winkler_upper_bound(&entry.profile) < 0.85 {
                return None;
            }
            let score = strsim::jaro_winkler(&lower, &entry.lower);
            (score >= 0.85).then_some((entry.entity_id, score.to_bits(), false))
        })
        .collect()
}

#[test]
#[ignore = "explicit private labels fixture only; no store/endpoint/model; not end-to-end performance"]
fn offline_existing_labels_score_bits_and_order() {
    let labels = PathBuf::from(
        std::env::var_os("QUIPU_RESOLUTION_LAB_LABELS").expect("explicit labels fixture required"),
    );
    let payload = PathBuf::from(
        std::env::var_os("QUIPU_RESOLUTION_LAB_PAYLOAD").expect("explicit node fixture required"),
    );
    let sample: usize = std::env::var("QUIPU_RESOLUTION_LAB_SAMPLE")
        .unwrap_or_else(|_| "1000".into())
        .parse()
        .unwrap();
    assert!((1..=1000).contains(&sample));
    let mut entries = Vec::new();
    for line in std::fs::read_to_string(labels).unwrap().lines() {
        let (id, data) = line.split_once('|').unwrap();
        let bytes = hex::decode(data).unwrap();
        if bytes.first() != Some(&1) {
            continue;
        }
        let lower = String::from_utf8(bytes[1..].to_vec())
            .unwrap()
            .to_lowercase();
        entries.push(Arc::new(LabelEntry {
            entity_id: id.parse().unwrap(),
            profile: LabelProfile::of(&lower),
            lower,
        }));
    }
    let index = LabelIndex { entries };
    assert!(index.entries.len() >= 600_000, "wrong fixture coverage");
    let fixture: serde_json::Value =
        serde_json::from_slice(&std::fs::read(payload).unwrap()).unwrap();
    let mut names: Vec<String> = fixture["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n["name"].as_str().unwrap().into())
        .collect();
    let fixture_count = names.len();
    // Fixed seed and stride, reproducible across runs over this exact label file.
    let mut seed = 720_261_021u64;
    for _ in 0..sample {
        seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
        let position = usize::try_from(seed % u64::try_from(index.entries.len()).unwrap()).unwrap();
        names.push(index.entries[position].lower.clone());
    }
    let mut old_seconds = 0.0;
    let mut new_seconds = 0.0;
    for (number, name) in names.iter().enumerate() {
        let start = Instant::now();
        let expected = old_candidates(&index, name);
        old_seconds += start.elapsed().as_secs_f64();
        let start = Instant::now();
        let actual: Vec<_> = index
            .candidates(name, 0.85)
            .unwrap()
            .into_iter()
            .map(|c| match c {
                Candidate::Name {
                    entity_id,
                    score,
                    exact,
                } => (entity_id, score.to_bits(), exact),
                Candidate::Known(_) => panic!("label scanner returned a non-label candidate"),
            })
            .collect();
        new_seconds += start.elapsed().as_secs_f64();
        assert_eq!(
            actual, expected,
            "case {number}: candidate IDs, exact flags, score bits or input order changed"
        );
        if number % 25 == 0 {
            println!("LAB_PROGRESS cases={} total={}", number + 1, names.len());
        }
    }
    println!(
        "LAB_PARITY {}",
        serde_json::json!({"labels":index.entries.len(),"fixture_names":fixture_count,"sampled_existing_labels":sample,"cases":names.len(),"old_scan_seconds":old_seconds,"new_scan_seconds":new_seconds,"exact_score_bits_and_order":true,"scope":"label scanner only; no SQL IRI resolution, startup or embedding; not end-to-end performance"})
    );
}
