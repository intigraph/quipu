//! Private frozen-copy index experiment; no service or production activation.
use quipu::Value;
use rusqlite::{Connection, OpenFlags, params};
use std::collections::BTreeSet;
use std::time::{Duration, Instant};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args().nth(1).ok_or("owned copy path required")?;
    if std::env::args().nth(2).as_deref() == Some("--verify") {
        return verify(&path);
    }
    if std::env::args().nth(2).as_deref() == Some("--provision") {
        let store = quipu::Store::open(&path)?;
        let started = Instant::now();
        for batch in 0..100 {
            if started.elapsed() > Duration::from_secs(170) {
                return Err("provision deadline exceeded".into());
            }
            let progress = store.backfill_substring_batch(10_000)?;
            if progress.complete {
                if !store.substring_index_ready()? {
                    return Err("complete index not admitted".into());
                }
                println!(
                    "{}",
                    serde_json::json!({"complete":true,"batches":batch+1,"elapsed_seconds":started.elapsed().as_secs_f64(),"progress":progress})
                );
                return Ok(());
            }
        }
        return Err("bounded batch census exceeded".into());
    }
    let mut conn = Connection::open(path)?;
    let started = Instant::now();
    conn.execute_batch("PRAGMA cache_size=-32000; PRAGMA temp_store=FILE;
        CREATE VIRTUAL TABLE wu_substring_fts USING fts5(text, content='', tokenize='trigram case_sensitive 1');")?;
    let tx = conn.transaction()?;
    let mut scanned = 0usize;
    let mut indexed = 0usize;
    {
        let mut read =
            tx.prepare("SELECT rowid,v FROM facts WHERE op=1 AND valid_to IS NULL ORDER BY rowid")?;
        let mut write = tx.prepare("INSERT INTO wu_substring_fts(rowid,text) VALUES(?1,?2)")?;
        let mut rows = read.query([])?;
        while let Some(row) = rows.next()? {
            scanned += 1;
            if scanned > 845_844 || started.elapsed() > Duration::from_secs(170) {
                return Err("finite input/deadline exceeded".into());
            }
            let bytes: Vec<u8> = row.get(1)?;
            let text = match Value::from_bytes(&bytes)? {
                Value::Str(s)
                | Value::Lang { lexical: s, .. }
                | Value::Typed { lexical: s, .. } => s,
                _ => continue,
            };
            // A NUL in a stored string must not hide a later substring from
            // SQLite's tokenizer. Queries containing NUL require fallback.
            write.execute(params![
                row.get::<_, i64>(0)?,
                text.to_lowercase().replace('\0', " ")
            ])?;
            indexed += 1;
        }
    }
    tx.commit()?;
    println!(
        "{}",
        serde_json::json!({"complete":true,"scanned":scanned,"indexed":indexed,"elapsed_seconds":started.elapsed().as_secs_f64()})
    );
    Ok(())
}

fn verify(path: &str) -> Result<(), Box<dyn std::error::Error>> {
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    conn.execute_batch("PRAGMA query_only=ON; PRAGMA cache_size=-32000;")?;
    let needles = [
        "quipu",
        "seeds",
        "source",
        "freeze",
        "rollback",
        "zz_no_seed_term_1514_control",
    ];
    let mut expected = vec![BTreeSet::new(); needles.len()];
    let started = Instant::now();
    let mut read =
        conn.prepare("SELECT rowid,v FROM facts WHERE op=1 AND valid_to IS NULL ORDER BY rowid")?;
    let mut rows = read.query([])?;
    let mut scanned = 0usize;
    while let Some(row) = rows.next()? {
        scanned += 1;
        if scanned > 845_844 || started.elapsed() > Duration::from_secs(110) {
            return Err("verification budget exceeded".into());
        }
        let text = match Value::from_bytes(&row.get::<_, Vec<u8>>(1)?)? {
            Value::Str(s) | Value::Lang { lexical: s, .. } | Value::Typed { lexical: s, .. } => {
                s.to_lowercase()
            }
            _ => continue,
        };
        for (i, needle) in needles.iter().enumerate() {
            if text.contains(needle) {
                expected[i].insert(row.get::<_, i64>(0)?);
            }
        }
    }
    for (i, needle) in needles.iter().enumerate() {
        let phrase = format!("\"{}\"", needle.replace('"', "\"\""));
        let mut search =
            conn.prepare("SELECT rowid FROM wu_substring_fts WHERE wu_substring_fts MATCH ?1")?;
        let found: BTreeSet<i64> = search
            .query_map([phrase], |row| row.get(0))?
            .collect::<Result<_, _>>()?;
        let missing = expected[i].difference(&found).count();
        println!(
            "{}",
            serde_json::json!({"term":needle,"exact_matches":expected[i].len(),"index_candidates":found.len(),"missing":missing,"false_positives":found.difference(&expected[i]).count()})
        );
        if missing != 0 || (i < 5 && expected[i].is_empty()) || (i == 5 && !found.is_empty()) {
            return Err("candidate coverage control failed".into());
        }
    }
    println!(
        "{}",
        serde_json::json!({"verified":true,"scanned":scanned,"elapsed_seconds":started.elapsed().as_secs_f64()})
    );
    Ok(())
}

#[test]
fn unicode_trigrams_keep_every_exact_substring_candidate() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE VIRTUAL TABLE f USING fts5(text,content='',tokenize='trigram case_sensitive 1')",
    )
    .unwrap();
    let documents = [
        "Straße Éclair İstanbul",
        "alpha\0Quipu",
        "quoted \"word\"",
        "🐟 fish and source",
        "unrelated",
    ];
    for (id, text) in documents.iter().enumerate() {
        conn.execute(
            "INSERT INTO f(rowid,text) VALUES(?1,?2)",
            params![id as i64 + 1, text.to_lowercase().replace('\0', " ")],
        )
        .unwrap();
    }
    for needle in [
        "straße",
        "écl",
        "i\u{307}st",
        "quipu",
        "\"word\"",
        "🐟 f",
        "source",
        "zz_no_hit_control",
    ] {
        let phrase = format!("\"{}\"", needle.replace('"', "\"\""));
        let mut statement = conn
            .prepare("SELECT rowid FROM f WHERE f MATCH ?1")
            .unwrap();
        let found: Vec<i64> = statement
            .query_map([phrase], |row| row.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        for (id, text) in documents.iter().enumerate() {
            if text.to_lowercase().contains(needle) {
                assert!(
                    found.contains(&(id as i64 + 1)),
                    "false negative: {needle:?} in {text:?}"
                );
            }
        }
        if needle == "zz_no_hit_control" {
            assert!(found.is_empty());
        }
    }
}
