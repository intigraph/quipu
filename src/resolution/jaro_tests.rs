use super::Matcher;
use crate::resolution::LabelProfile;
use proptest::prelude::*;

fn pair(a: &str, b: &str) {
    let (a, b) = (a.to_lowercase(), b.to_lowercase());
    let expected = strsim::jaro_winkler(&a, &b);
    let prefix = a
        .chars()
        .zip(b.chars())
        .take(4)
        .take_while(|(a, b)| a == b)
        .count();
    let upper = LabelProfile::of(&a).upper_bound_with_prefix(&LabelProfile::of(&b), prefix);
    assert!(
        upper >= expected,
        "prefix bound lost {a:?}/{b:?}: {upper}/{expected}"
    );
    let mut matcher = Matcher::new(&a);
    for threshold in [0.0, 0.7, 0.85, expected, 1.0] {
        let actual = matcher.score(&b, threshold);
        if expected >= threshold {
            assert!(
                actual.is_some(),
                "early exit lost {a:?}/{b:?} at {threshold}"
            );
        }
        if let Some(actual) = actual {
            assert_eq!(actual.to_bits(), expected.to_bits(), "{a:?}/{b:?}");
        }
    }
}

#[test]
fn exact_boundaries_unicode_transpositions_and_buffer_reuse() {
    for (a, b) in [
        ("", ""),
        ("", "x"),
        ("x", ""),
        ("a", "a"),
        ("abcd", "abdc"),             // Clears 0.85 without a shared trigram.
        ("aaaaaaaaaa", "aaaaaaabbb"), // Real score exactly 0.88.
        ("MARTHA", "MARHTA"),
        ("DIXON", "DICKSONX"),
        ("İstanbul ΩΣ", "i\u{307}stanbul ωσ"),
    ] {
        pair(a, b);
    }
    pair(&"a".repeat(300), &format!("{}b", "a".repeat(299)));
    let mut matcher = Matcher::new("abcd");
    for b in ["abdc", "", "zzzzzzzzzzzz", "abcd", "a", "abdc"] {
        assert_eq!(
            matcher.score(b, 0.0).unwrap().to_bits(),
            strsim::jaro_winkler("abcd", b).to_bits()
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(192))]
    #[test]
    fn retained_scores_and_prefix_bounds_match_strsim(
        a in "[a-zA-Z0-9À-ÿΩΣ _/-]{0,80}",
        b in "[a-zA-Z0-9À-ÿΩΣ _/-]{0,80}",
    ) { pair(&a, &b); }
}
