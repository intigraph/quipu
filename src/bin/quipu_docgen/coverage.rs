//! Agent-surface coverage: every top-level CLI verb and every MCP tool is
//! named in the primer (`src/primer.md`) or the quipu skill, or is listed in
//! `skills/quipu/surface-exclusions.txt` with a reason.
//!
//! The point is that a new verb or tool forces a decision. Either agents
//! learn about it, or someone writes down why they need not. The exclusion
//! list is kept tight in both directions: an entry that names nothing, or
//! names something the primer or skill already mentions, is an error too.

use std::collections::BTreeSet;
use std::path::Path;

use serde_json::Value;

use crate::catalogue::SPECS;

pub const PRIMER: &str = "src/primer.md";
pub const SKILL: &str = "skills/quipu/SKILL.md";
pub const EXCLUSIONS: &str = "skills/quipu/surface-exclusions.txt";

/// `(kind, name)` for every surface item: `("cli", verb)` and `("mcp", tool)`.
pub fn surface(tools: &[Value]) -> BTreeSet<(&'static str, String)> {
    let mut items = BTreeSet::new();
    for spec in SPECS {
        if let Some(verb) = spec.path.split_whitespace().next() {
            items.insert(("cli", verb.to_string()));
        }
    }
    for tool in tools {
        if let Some(name) = tool["name"].as_str() {
            items.insert(("mcp", name.to_string()));
        }
    }
    items
}

/// Whether `text` names the item. A verb counts as `quipu <verb>` or
/// `qp <verb>`; a tool counts by its exact name, so `quipu_search` is not
/// mentioned by `quipu_search_nodes`.
pub fn mentioned(text: &str, kind: &str, name: &str) -> bool {
    let needles: Vec<String> = if kind == "cli" {
        vec![format!("quipu {name}"), format!("qp {name}")]
    } else {
        vec![name.to_string()]
    };
    needles.iter().any(|needle| {
        text.match_indices(needle.as_str()).any(|(i, _)| {
            let before = text[..i].chars().next_back();
            let after = text[i + needle.len()..].chars().next();
            let word = |c: char| c.is_ascii_alphanumeric() || c == '_' || c == '-';
            !before.is_some_and(word) && !after.is_some_and(word)
        })
    })
}

/// Parse `kind name reason...` lines; `#` starts a comment.
pub fn parse_exclusions(text: &str) -> Result<Vec<(String, String)>, String> {
    let mut out = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let mut parts = line.splitn(3, char::is_whitespace);
        match (parts.next(), parts.next(), parts.next()) {
            (Some(kind @ ("cli" | "mcp")), Some(name), Some(reason))
                if !reason.trim().is_empty() =>
            {
                out.push((kind.to_string(), name.to_string()));
            }
            _ => {
                return Err(format!(
                    "{EXCLUSIONS}:{}: expected `cli|mcp <name> <reason>`",
                    n + 1
                ));
            }
        }
    }
    Ok(out)
}

/// Problems with coverage, as a list of messages; empty means covered.
pub fn check(root: &Path, tools: &[Value]) -> Vec<String> {
    let read = |rel: &str| std::fs::read_to_string(root.join(rel));
    let (Ok(primer), Ok(skill)) = (read(PRIMER), read(SKILL)) else {
        return vec![format!("cannot read {PRIMER} or {SKILL}")];
    };
    let text = format!("{primer}\n{skill}");
    let exclusions = match parse_exclusions(&read(EXCLUSIONS).unwrap_or_default()) {
        Ok(e) => e,
        Err(e) => return vec![e],
    };
    let items = surface(tools);
    let mut problems = Vec::new();
    for (kind, name) in &items {
        let excluded = exclusions.iter().any(|(k, n)| k == kind && n == name);
        if !excluded && !mentioned(&text, kind, name) {
            problems.push(format!(
                "uncovered {kind} {name}: name it in {PRIMER} (then bless the skill copy) \
                 or add `{kind} {name} <reason>` to {EXCLUSIONS}"
            ));
        }
    }
    for (kind, name) in &exclusions {
        if !items.iter().any(|(k, n)| k == kind && n == name) {
            problems.push(format!(
                "stale exclusion {kind} {name}: no such {kind} item"
            ));
        } else if mentioned(&text, kind, name) {
            problems.push(format!(
                "redundant exclusion {kind} {name}: the primer or skill already names it"
            ));
        }
    }
    problems
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mentions_respect_word_boundaries() {
        assert!(mentioned(
            "call `quipu_search` first",
            "mcp",
            "quipu_search"
        ));
        assert!(!mentioned("quipu_search_nodes", "mcp", "quipu_search"));
        assert!(mentioned("run qp knot data.ttl", "cli", "knot"));
        assert!(!mentioned("quipu knotted", "cli", "knot"));
        assert!(!mentioned("quipu import-review", "cli", "import"));
    }

    #[test]
    fn exclusions_need_a_reason() {
        assert!(parse_exclusions("cli repl interactive only\n# c\n").is_ok());
        assert!(parse_exclusions("cli repl\n").is_err());
        assert!(parse_exclusions("tool x why\n").is_err());
    }

    #[test]
    fn the_repository_is_covered() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let problems = check(root, &quipu::mcp::tool_definitions());
        assert!(problems.is_empty(), "{}", problems.join("\n"));
    }

    #[test]
    fn a_new_tool_is_uncovered_until_named_or_excluded() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut tools = quipu::mcp::tool_definitions();
        tools.push(serde_json::json!({"name": "quipu_brand_new"}));
        let problems = check(root, &tools);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("uncovered mcp quipu_brand_new"));
    }
}
