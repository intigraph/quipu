//! `quipu prime`: the agent primer, plus a bounded summary of the local store.
//!
//! The static half is `src/primer.md`, embedded at build time. It is the one
//! source for agent orientation: the quipu skill carries a generated copy of it
//! and the book includes it, and `tests/prime_sync.rs` fails when either copy
//! drifts.
//!
//! The live half never writes and never creates anything. The store is opened
//! `SQLITE_OPEN_READ_ONLY` with `query_only`, every query is a small indexed
//! read with a row cap, and a progress handler interrupts the whole summary
//! once [`DEADLINE`] passes, so a huge or locked store costs at most that long.
//! A missing store, an older schema or an interrupted read degrades to the
//! static primer with a note; it is never an error.

use std::time::{Duration, Instant};

use rusqlite::{Connection, OpenFlags};
use serde_json::{Value, json};

/// The static primer. One source: the skill and the book carry copies of it.
pub(crate) const PRIMER: &str = include_str!("primer.md");

/// Wall-clock budget for the live summary, well inside the 1 s target.
const DEADLINE: Duration = Duration::from_millis(400);

/// Most graph / shape / ontology names listed before eliding the rest.
const LIST_CAP: usize = 20;

struct Options<'a> {
    brief: bool,
    section: Option<&'a str>,
    json: bool,
    offline: bool,
}

fn parse(args: &[String]) -> Options<'_> {
    let has = |flag: &str| args.iter().any(|a| a == flag);
    Options {
        brief: has("--brief"),
        section: args
            .windows(2)
            .find(|w| w[0] == "--section")
            .map(|w| w[1].as_str()),
        json: has("--json"),
        offline: has("--static"),
    }
}

/// Section headings of the primer (`## ...`), in order.
pub(crate) fn sections(primer: &str) -> Vec<&str> {
    primer
        .lines()
        .filter_map(|l| l.strip_prefix("## "))
        .collect()
}

/// The primer text to print: whole, brief (the first two sections), or one
/// section chosen by a case-insensitive heading prefix.
pub(crate) fn select(primer: &str, brief: bool, section: Option<&str>) -> Result<String, String> {
    let blocks = split(primer);
    if let Some(wanted) = section {
        let needle = wanted.trim().to_lowercase();
        let found = blocks
            .iter()
            .skip(1)
            .find(|(heading, _)| heading.to_lowercase().starts_with(&needle));
        return found.map(|(_, body)| (*body).to_string()).ok_or_else(|| {
            format!(
                "unknown primer section: {wanted}. Sections: {}",
                sections(primer).join(", ")
            )
        });
    }
    if brief {
        return Ok(blocks.iter().take(3).map(|(_, b)| *b).collect());
    }
    Ok(primer.to_string())
}

/// `(heading, text)` blocks; block 0 is everything before the first `## `.
fn split(primer: &str) -> Vec<(&str, &str)> {
    let mut starts = vec![(0usize, "")];
    let mut offset = 0;
    for line in primer.split_inclusive('\n') {
        if let Some(h) = line.strip_prefix("## ") {
            starts.push((offset, h.trim_end()));
        }
        offset += line.len();
    }
    starts
        .iter()
        .enumerate()
        .map(|(i, &(start, heading))| {
            let end = starts.get(i + 1).map_or(primer.len(), |s| s.0);
            (heading, &primer[start..end])
        })
        .collect()
}

/// A bounded, read-only summary of the store at `path`.
pub(crate) fn summarize(path: &str) -> Value {
    let started = Instant::now();
    let meta = match std::fs::metadata(path) {
        Ok(m) if m.is_file() => m,
        _ => return json!({ "path": path, "status": "absent" }),
    };
    let mut out = match read_summary(path, started) {
        Ok(v) => v,
        Err(rusqlite::Error::InvalidQuery) => json!({
            "status": "unreadable",
            "error": "not a quipu store: no summary table could be read",
        }),
        Err(e) => json!({ "status": "unreadable", "error": e.to_string() }),
    };
    out["path"] = json!(path);
    out["bytes"] = json!(meta.len());
    out["elapsed_ms"] = json!(started.elapsed().as_millis());
    out
}

fn read_summary(path: &str, started: Instant) -> rusqlite::Result<Value> {
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    conn.pragma_update(None, "query_only", "ON")?;
    conn.busy_timeout(Duration::from_millis(100))?;
    conn.progress_handler(1000, Some(move || started.elapsed() > DEADLINE))?;

    let last_tx = conn
        .query_row(
            "SELECT id, timestamp FROM transactions ORDER BY id DESC LIMIT 1",
            [],
            |r| Ok(json!({ "id": r.get::<_, i64>(0)?, "timestamp": r.get::<_, String>(1)? })),
        )
        .ok();
    let graphs = names(
        &conn,
        "SELECT COALESCE(t.iri, 'g' || g.g) || CASE WHEN g.class = 'overlay' \
         THEN ' (overlay)' ELSE '' END FROM graphs g LEFT JOIN terms t ON t.id = g.g \
         WHERE g.g != 0 ORDER BY 1",
        "SELECT COUNT(*) FROM graphs WHERE g != 0",
    );
    let shapes = names(
        &conn,
        "SELECT name FROM shapes ORDER BY name",
        "SELECT COUNT(*) FROM shapes",
    );
    let ontologies = names(
        &conn,
        "SELECT name FROM ontologies ORDER BY name",
        "SELECT COUNT(*) FROM ontologies",
    );
    let pending = conn
        .query_row(
            "SELECT COUNT(*) FROM proposals WHERE status = 'pending'",
            [],
            |r| r.get::<_, i64>(0),
        )
        .ok();
    // Open succeeds lazily on any file; a file that is not a quipu store
    // (or not SQLite at all) fails every read, which is "unreadable", not an
    // "ok" store with every field empty.
    let nothing_read = last_tx.is_none()
        && graphs.is_null()
        && shapes.is_null()
        && ontologies.is_null()
        && pending.is_none();
    if nothing_read && started.elapsed() <= DEADLINE {
        return Err(rusqlite::Error::InvalidQuery);
    }
    let status = if started.elapsed() > DEADLINE {
        "partial"
    } else {
        "ok"
    };
    Ok(json!({
        "status": status,
        "last_tx": last_tx,
        "graphs": graphs,
        "shapes": shapes,
        "ontologies": ontologies,
        "pending_proposals": pending,
    }))
}

/// `{count, names}` with at most [`LIST_CAP`] names, or null when the table
/// is missing (an older store) or the deadline interrupted the read.
fn names(conn: &Connection, list_sql: &str, count_sql: &str) -> Value {
    let count = conn.query_row(count_sql, [], |r| r.get::<_, i64>(0)).ok();
    let list = conn
        .prepare(&format!("{list_sql} LIMIT {LIST_CAP}"))
        .and_then(|mut stmt| {
            stmt.query_map([], |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()
        })
        .ok();
    match (count, list) {
        (Some(count), Some(names)) => json!({ "count": count, "names": names }),
        _ => Value::Null,
    }
}

/// The live summary as primer-style Markdown.
pub(crate) fn render_summary(summary: &Value) -> String {
    let path = summary["path"].as_str().unwrap_or("?");
    let mut out = String::from("\n## Live store\n\n");
    match summary["status"].as_str() {
        Some("absent") => {
            out.push_str(&format!(
                "No store at `{path}`. Pass `--db <path>`, or create one with `quipu knot`.\n"
            ));
            return out;
        }
        Some("unreadable") => {
            out.push_str(&format!(
                "Store at `{path}` could not be read: {}\n",
                summary["error"].as_str().unwrap_or("unknown error")
            ));
            return out;
        }
        _ => {}
    }
    out.push_str(&format!("- Store: `{path}`\n"));
    if let Some(tx) = summary["last_tx"].as_object() {
        out.push_str(&format!(
            "- Last transaction: {} at {}\n",
            tx["id"],
            tx["timestamp"].as_str().unwrap_or("?")
        ));
    }
    for (key, label) in [
        ("graphs", "Named graphs"),
        ("shapes", "Shapes"),
        ("ontologies", "Ontologies"),
    ] {
        let Some(count) = summary[key]["count"].as_i64() else {
            continue;
        };
        let names: Vec<String> = summary[key]["names"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|n| n.as_str().map(|s| format!("`{s}`")))
            .collect();
        let more = usize::try_from(count)
            .unwrap_or(0)
            .saturating_sub(names.len());
        let tail = if more > 0 {
            format!(" and {more} more")
        } else {
            String::new()
        };
        if names.is_empty() {
            out.push_str(&format!("- {label}: {count}\n"));
        } else {
            out.push_str(&format!(
                "- {label}: {count} ({}{tail})\n",
                names.join(", ")
            ));
        }
    }
    if let Some(n) = summary["pending_proposals"].as_i64().filter(|n| *n > 0) {
        out.push_str(&format!(
            "- Pending schema proposals: {n} (`quipu propose list`)\n"
        ));
    }
    if summary["status"] == "partial" {
        out.push_str("- Summary cut short by its time budget; counts above may be missing.\n");
    }
    out
}

/// Entry point: `quipu prime [--brief] [--section NAME] [--static] [--json] [--db PATH]`.
pub fn cmd_prime(args: &[String], db_path: &str) {
    let opts = parse(args);
    let text = select(PRIMER, opts.brief, opts.section).unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(2);
    });
    let summary = (!opts.offline).then(|| summarize(db_path));
    if opts.json {
        let value = json!({ "primer": text, "store": summary });
        println!(
            "{}",
            serde_json::to_string_pretty(&value).unwrap_or_default()
        );
        return;
    }
    print!("{text}");
    if let Some(summary) = summary {
        print!("{}", render_summary(&summary));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn primer_has_the_sections_agents_rely_on() {
        let s = sections(PRIMER);
        for wanted in [
            "What Quipu is",
            "Query first",
            "Core commands",
            "MCP tools",
            "Writing safely",
        ] {
            assert!(s.contains(&wanted), "primer lost section {wanted:?}: {s:?}");
        }
    }

    #[test]
    fn select_by_section_prefix_is_case_insensitive() {
        let mcp = select(PRIMER, false, Some("mcp")).unwrap();
        assert!(mcp.starts_with("## MCP tools"));
        assert!(!mcp.contains("## Writing safely"));
        assert!(select(PRIMER, false, Some("nope")).is_err());
    }

    #[test]
    fn brief_is_title_plus_first_two_sections() {
        let brief = select(PRIMER, true, None).unwrap();
        assert!(brief.contains("## What Quipu is"));
        assert!(brief.contains("## Query first"));
        assert!(!brief.contains("## Core commands"));
        assert!(PRIMER.starts_with(&brief));
    }

    #[test]
    fn a_file_that_is_not_a_store_is_unreadable() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("garbage.db");
        std::fs::write(
            &path,
            b"this is not sqlite at all, just bytes\n".repeat(200),
        )
        .unwrap();
        let s = summarize(path.to_str().unwrap());
        assert_eq!(s["status"], "unreadable", "{s}");
        assert!(render_summary(&s).contains("could not be read"));
        // A valid SQLite file without quipu's tables is not a store either.
        // (A pragma, not a table: share_completeness scans the source for
        // every table definition.)
        let empty = dir.path().join("empty.db");
        rusqlite::Connection::open(&empty)
            .unwrap()
            .execute_batch("PRAGMA user_version = 7")
            .unwrap();
        assert!(std::fs::metadata(&empty).unwrap().len() > 0);
        assert_eq!(summarize(empty.to_str().unwrap())["status"], "unreadable");
    }

    #[test]
    fn absent_store_is_reported_not_created() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("missing").join("quipu.db");
        let s = summarize(path.to_str().unwrap());
        assert_eq!(s["status"], "absent");
        assert!(!path.exists() && !path.parent().unwrap().exists());
        assert!(render_summary(&s).contains("No store at"));
    }
}
