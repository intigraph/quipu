//! `src/primer.md` is the one source for agent orientation (`quipu prime`).
//!
//! The quipu skill carries a generated copy between markers, and the book
//! includes the file directly. This test fails when the skill's copy drifts;
//! `QUIPU_BLESS_GOLDENS=1 cargo test --test prime_sync` rewrites it.

use std::path::{Path, PathBuf};

const BEGIN: &str = "<!-- BEGIN quipu prime";
const END: &str = "<!-- END quipu prime -->";

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

/// The primer as the skill embeds it: every heading one level deeper, so the
/// primer's title nests under the skill's own `## Agent primer`.
fn embedded(primer: &str) -> String {
    primer
        .lines()
        .map(|l| {
            if l.starts_with('#') {
                format!("##{l}")
            } else {
                l.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn skill_carries_the_current_primer() {
    let primer = std::fs::read_to_string(root().join("src/primer.md")).unwrap();
    let skill_path = root().join("skills/quipu/SKILL.md");
    let skill = std::fs::read_to_string(&skill_path).unwrap();
    let start = skill
        .find(BEGIN)
        .expect("skill lost its BEGIN quipu prime marker");
    let body_start = start + skill[start..].find('\n').unwrap() + 1;
    let end = skill
        .find(END)
        .expect("skill lost its END quipu prime marker");
    let want = format!("{}\n", embedded(&primer));
    if skill[body_start..end] == want {
        return;
    }
    if std::env::var_os("QUIPU_BLESS_GOLDENS").is_some() {
        let fixed = format!("{}{want}{}", &skill[..body_start], &skill[end..]);
        std::fs::write(&skill_path, fixed).unwrap();
        return;
    }
    panic!(
        "skills/quipu/SKILL.md carries a stale copy of src/primer.md. Run \
         QUIPU_BLESS_GOLDENS=1 cargo test --test prime_sync and commit the result."
    );
}

#[test]
fn book_includes_the_primer_file() {
    let page =
        std::fs::read_to_string(root().join("docs/book/src/getting-started/agent-primer.md"))
            .unwrap();
    assert!(page.contains("{{#include ../../../../src/primer.md}}"));
    let summary = std::fs::read_to_string(root().join("docs/book/src/SUMMARY.md")).unwrap();
    assert!(summary.contains("(getting-started/agent-primer.md)"));
}
