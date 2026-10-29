//! The fixed text and markdown layouts of a [`MergeView`] (aegis-k7l).

#[allow(clippy::wildcard_imports)]
use super::*;

pub(super) fn values_line(values: &[String]) -> String {
    if values.is_empty() {
        "(none)".into()
    } else {
        values.join(", ")
    }
}

pub(super) fn header(v: &MergeView) -> (String, String) {
    let n = v.conflicts.len();
    (
        if v.undecided > 0 {
            format!("Merge needs your decision: {}", v.merge.title)
        } else {
            format!("Merge decided, ready to apply: {}", v.merge.title)
        },
        format!(
            "{n} conflict{}{}, {} undecided. Nothing has been written.",
            if n == 1 { "" } else { "s" },
            match v.aliases.len() {
                0 => String::new(),
                1 => ", 1 alias proposal".into(),
                a => format!(", {a} alias proposals"),
            },
            v.undecided
        ),
    )
}

pub(super) fn title(c: &ConflictCard) -> String {
    let name = |n: &Named| n.label.clone().unwrap_or_else(|| n.iri.clone());
    format!("{} · {}", name(&c.entity), name(&c.field))
}

pub(super) fn decided_line(c: &ConflictCard) -> String {
    match (&c.decision, &c.decided_by) {
        (Some(d), Some(by)) => format!("{d} (by {by})"),
        (Some(d), None) => d.clone(),
        (None, Some(by)) => format!("(deferred by {by})"),
        (None, None) => "(none)".into(),
    }
}

/// The text layout (`quipu merge show`, the default).
#[must_use]
pub fn render_text(v: &MergeView) -> String {
    let (h1, h2) = header(v);
    let mut s = format!("{h1}\n{h2}\n");
    for c in &v.conflicts {
        s.push_str(&format!("\n[{}] {}\n", c.id, title(c)));
        s.push_str(&format!("  rule     {}\n", c.rule.text));
        s.push_str(&format!("  base     {}\n", values_line(&c.base)));
        for (name, side) in [("ours", &c.ours), ("theirs", &c.theirs)] {
            let by = if side.changed_by.is_empty() {
                String::new()
            } else {
                format!("  ({})", side.changed_by.join("; "))
            };
            s.push_str(&format!("  {name:<8} {}{by}\n", values_line(&side.values)));
        }
        if let Some(sg) = &c.suggestion {
            s.push_str(&format!("  suggest  {}: {}\n", sg.choose, sg.reason));
        }
        let numbered: Vec<String> = c
            .choices
            .iter()
            .enumerate()
            .map(|(i, ch)| format!("{}) {ch}", i + 1))
            .collect();
        s.push_str(&format!("  choose   {}\n", numbered.join("  ")));
        s.push_str(&format!("  decided  {}\n", decided_line(c)));
    }
    for a in &v.aliases {
        s.push_str(&format!(
            "\n[{}] same entity? {} and {} (label similarity {:.3})\n",
            a.id,
            alias_name(&a.ours),
            alias_name(&a.theirs),
            a.similarity
        ));
        let numbered: Vec<String> = a
            .choices
            .iter()
            .enumerate()
            .map(|(i, ch)| format!("{}) {ch}", i + 1))
            .collect();
        s.push_str(&format!("  choose   {}\n", numbered.join("  ")));
        s.push_str(&format!("  decided  {}\n", alias_decided(a)));
    }
    s.push('\n');
    for (i, step) in v.next.iter().enumerate() {
        s.push_str(&format!(
            "{} {step}\n",
            if i == 0 { "Next:" } else { "Then:" }
        ));
    }
    s
}

pub(super) fn alias_name(n: &Named) -> String {
    match &n.label {
        Some(l) => format!("{} ({l})", n.iri),
        None => n.iri.clone(),
    }
}

pub(super) fn alias_decided(a: &AliasCard) -> String {
    match (&a.decision, &a.decided_by) {
        (Some(d), Some(by)) => format!("{d} (by {by})"),
        (Some(d), None) => d.clone(),
        (None, Some(by)) => format!("(deferred by {by})"),
        (None, None) => "(none)".into(),
    }
}

pub(super) fn code(v: &str) -> String {
    format!("`{}`", cell(&v.replace('`', "'")))
}

pub(super) fn cell(v: &str) -> String {
    v.replace('|', "\\|").replace(['\n', '\r'], " ")
}

pub(super) fn code_list(values: &[String]) -> String {
    if values.is_empty() {
        "(none)".into()
    } else {
        values
            .iter()
            .map(|v| code(v))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// The markdown layout (`--format md`, MCP `presentation`, chat).
#[must_use]
pub fn render_markdown(v: &MergeView) -> String {
    let (h1, h2) = header(v);
    let (lead, rest) = h1.split_once(": ").unwrap_or((&h1, ""));
    let mut s = format!("**{lead}:** {}\n{h2}\n", cell(rest));
    for c in &v.conflicts {
        s.push_str(&format!("\n#### {}. {}\n\n", c.id, cell(&title(c))));
        s.push_str("| | Value | Changed by |\n|---|---|---|\n");
        s.push_str(&format!("| base | {} | |\n", code_list(&c.base)));
        for (name, side) in [("ours", &c.ours), ("theirs", &c.theirs)] {
            s.push_str(&format!(
                "| {name} | {} | {} |\n",
                code_list(&side.values),
                cell(&side.changed_by.join("; "))
            ));
        }
        s.push_str(&format!("\nRule: {}\n", cell(&c.rule.text)));
        if let Some(sg) = &c.suggestion {
            s.push_str(&format!(
                "Suggested: **{}**: {}\n",
                sg.choose,
                cell(&sg.reason)
            ));
        }
        s.push_str(&format!("Decided: {}\n\n", cell(&decided_line(c))));
        for (i, ch) in c.choices.iter().enumerate() {
            s.push_str(&format!("{}. {ch}\n", i + 1));
        }
    }
    for a in &v.aliases {
        s.push_str(&format!(
            "\n#### {}. Same entity? {} and {}\n\nLabel similarity {:.3}. Accept adds `owl:sameAs`.\nDecided: {}\n\n",
            a.id,
            cell(&alias_name(&a.ours)),
            cell(&alias_name(&a.theirs)),
            a.similarity,
            cell(&alias_decided(a))
        ));
        for (i, ch) in a.choices.iter().enumerate() {
            s.push_str(&format!("{}. {ch}\n", i + 1));
        }
    }
    s.push('\n');
    for (i, step) in v.next.iter().enumerate() {
        s.push_str(&format!(
            "{} `{}`\n",
            if i == 0 { "Next:" } else { "Then:" },
            step.replace('`', "'")
        ));
    }
    s
}
