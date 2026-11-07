//! Durable frozen-pass queue and atomic private promotion.
use super::Snapshot;
use crate::error::{Error, Result};
use crate::{Datum, MaterializeReport, Op, Store, Value};
use rusqlite::params;
use serde::{Deserialize, Serialize};

fn engine() -> &'static str {
    static ID: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    ID.get_or_init(|| {
        use sha2::Digest;
        let mut hash = sha2::Sha256::new();
        for source in [
            include_str!("owl_materialize.rs"),
            include_str!("owl_pass.rs"),
            include_str!("owl_parse.rs"),
            include_str!("owl_snapshot.rs"),
            include_str!("owl_snapshot_apply.rs"),
            include_str!("owl_snapshot_pages.rs"),
            include_str!("types.rs"),
            include_str!("owl.rs"),
            include_str!("store/ops_transaction.rs"),
        ] {
            hash.update((source.len() as u64).to_le_bytes());
            hash.update(source.as_bytes());
        }
        format!("owl-frozen-pages-v1:{:x}", hash.finalize())
    })
}
const MAX_DISK_PAGES: usize = 2_097_152; // 8 GiB at the required 4096-byte SQLite page size.
const MAX_SECONDS: i64 = 1200;

#[derive(Serialize, Deserialize)]
pub(super) struct Job {
    engine: String,
    pub generation: String,
    pub identity: String,
    pub ontologies: Vec<(String, String, String)>,
    pub source_companion: Option<i64>,
    pub premise_head: i64,
    pub timestamp: String,
    pub phase: String,
    pub failure: Option<String>,
    pub pass: usize,
    pub cursor: i64,
    pub queue_count: usize,
    pub apply_cursor: i64,
    pub deadline: i64,
    pub report: MaterializeReport,
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(i64::MAX, |d| d.as_secs() as i64)
}
fn invalid(message: &str) -> Error {
    Error::InvalidValue(message.into())
}

pub(super) fn cardinality(value: i64) -> Result<usize> {
    usize::try_from(value).map_err(|_| invalid("OWL invalid queue cardinality"))
}

pub(super) fn limits(store: &Store) -> Result<()> {
    let page_size: i64 = store.conn.query_row("PRAGMA page_size", [], |r| r.get(0))?;
    let page_count: i64 = store
        .conn
        .query_row("PRAGMA page_count", [], |r| r.get(0))?;
    if page_size != 4096 || page_count > MAX_DISK_PAGES as i64 {
        return Err(invalid(
            "OWL scratch disk budget exceeded or page size unsupported",
        ));
    }
    store.conn.execute_batch(&format!("PRAGMA max_page_count={MAX_DISK_PAGES}; PRAGMA cache_size=-8192; PRAGMA mmap_size=0; PRAGMA temp_store=FILE;"))?;
    store.set_term_cache_limit(32_768);
    Ok(())
}

pub(super) fn initialize(snapshot: &Snapshot) -> Result<()> {
    limits(&snapshot.scratch)?;
    snapshot.scratch.conn.execute_batch(
        "CREATE TABLE owl_page_job(id INTEGER PRIMARY KEY CHECK(id=1), state TEXT NOT NULL);
         CREATE TABLE owl_page_queue(id INTEGER PRIMARY KEY,e INTEGER NOT NULL,a INTEGER NOT NULL,v BLOB NOT NULL,UNIQUE(e,a,v));
         CREATE TABLE owl_page_all(id INTEGER PRIMARY KEY,e INTEGER NOT NULL,a INTEGER NOT NULL,v BLOB NOT NULL,UNIQUE(e,a,v));"
    )?;
    let job = Job {
        engine: engine().into(),
        generation: snapshot.scratch.store_id()?,
        identity: snapshot.identity.clone(),
        ontologies: snapshot.ontologies.clone(),
        source_companion: snapshot.source_companion,
        premise_head: snapshot.premise_head,
        timestamp: snapshot.timestamp.clone(),
        phase: "CAPTURED".into(),
        failure: None,
        pass: 0,
        cursor: 0,
        queue_count: 0,
        apply_cursor: 0,
        deadline: now() + MAX_SECONDS,
        report: MaterializeReport::default(),
    };
    save(&snapshot.scratch, &job)
}

pub(super) fn load(store: &Store) -> Result<Job> {
    let state: String =
        store
            .conn
            .query_row("SELECT state FROM owl_page_job WHERE id=1", [], |r| {
                r.get(0)
            })?;
    let job: Job = serde_json::from_str(&state).map_err(|e| invalid(&e.to_string()))?;
    if job.engine != engine()
        || job.generation != store.store_id()?
        || job.ontologies != store.list_ontologies()?
    {
        return Err(invalid("OWL scratch generation/engine/ontology changed"));
    }
    Ok(job)
}

pub(super) fn save(store: &Store, job: &Job) -> Result<()> {
    let state = serde_json::to_string(job).map_err(|e| invalid(&e.to_string()))?;
    store.conn.execute("INSERT INTO owl_page_job(id,state) VALUES(1,?1) ON CONFLICT(id) DO UPDATE SET state=excluded.state", [state])?;
    Ok(())
}

pub(crate) fn check_budget(store: &Store) -> Result<()> {
    // The absolute deadline is persisted before enumeration and never renewed on resume.
    let deadline: i64 = store.conn.query_row(
        "SELECT json_extract(state,'$.deadline') FROM owl_page_job WHERE id=1",
        [],
        |r| r.get(0),
    )?;
    if now() >= deadline {
        return Err(invalid("OWL durable job deadline exceeded; incomplete"));
    }
    Ok(())
}

pub(super) fn read_page(
    store: &Store,
    table: &str,
    after: i64,
    limit: usize,
    timestamp: &str,
) -> Result<Vec<(i64, Datum)>> {
    if !matches!(table, "owl_page_queue" | "owl_page_all")
        || limit == 0
        || limit > super::MAX_PROPOSALS
    {
        return Err(invalid("OWL invalid proposal page"));
    }
    let mut stmt = store.conn.prepare(&format!(
        "SELECT id,e,a,v FROM {table} WHERE id>?1 ORDER BY id LIMIT ?2"
    ))?;
    let mut rows = stmt.query(params![after, limit as i64])?;
    let mut page = Vec::with_capacity(limit.min(1024));
    while let Some(row) = rows.next()? {
        page.push((
            row.get(0)?,
            Datum {
                entity: row.get(1)?,
                attribute: row.get(2)?,
                value: Value::from_bytes(&row.get::<_, Vec<u8>>(3)?)?,
                valid_from: timestamp.into(),
                valid_to: None,
                op: Op::Assert,
            },
        ));
    }
    Ok(page)
}

pub(super) fn derive(snapshot: &mut Snapshot, page_size: usize) -> Result<()> {
    let result = derive_inner(snapshot, page_size);
    if let Err(error) = &result {
        snapshot.scratch.term_cache.borrow_mut().clear_persistent();
        if let Error::InvalidValue(reason) = error
            && reason != "simulated crash"
            && let Ok(mut job) = load(&snapshot.scratch)
        {
            if !matches!(job.phase.as_str(), "FAILED" | "INVALIDATED") {
                job.phase = "FAILED".into();
                job.failure = Some(reason.clone());
            }
            let _ = save(&snapshot.scratch, &job);
        }
    }
    result
}

fn derive_inner(snapshot: &mut Snapshot, page_size: usize) -> Result<()> {
    if page_size == 0 || page_size > super::MAX_PROPOSALS {
        return Err(invalid("OWL invalid page size"));
    }
    let mut job = load(&snapshot.scratch)?;
    loop {
        check_budget(&snapshot.scratch)?;
        match job.phase.as_str() {
            "FIXPOINT_COMPLETE" | "APPLYING" | "COMPLETE" => {
                snapshot.report = job.report;
                snapshot.derived = true;
                return Ok(());
            }
            "CAPTURED" | "ENUMERATING" => {
                if job.pass >= 64 {
                    return Err(invalid("OWL logical pass budget exceeded; incomplete"));
                }
                let premises: i64 = snapshot.scratch.conn.query_row(
                    "SELECT count(*) FROM facts WHERE op=1 AND valid_to IS NULL",
                    [],
                    |r| r.get(0),
                )?;
                if premises > super::MAX_PREMISES as i64 {
                    return Err(invalid("OWL expanded premise budget exceeded; incomplete"));
                }
                job.phase = "ENUMERATING".into();
                save(&snapshot.scratch, &job)?;
                snapshot.scratch.ensure_owl_cache()?;
                let ontology = snapshot.scratch.owl_cache.as_deref().cloned();
                let companion = snapshot.scratch.intern(super::ROOT_INFERRED_GRAPH_IRI)?;
                let tx = snapshot.scratch.conn.unchecked_transaction()?;
                snapshot
                    .scratch
                    .conn
                    .execute("DELETE FROM owl_page_queue", [])?;
                let mut report = job.report.clone();
                if let Some(ontology) = ontology {
                    ontology.derive_queued_pass(
                        &snapshot.scratch,
                        companion,
                        &job.timestamp,
                        &mut report,
                    )?;
                }
                let count: i64 = snapshot.scratch.conn.query_row(
                    "SELECT count(*) FROM owl_page_queue",
                    [],
                    |r| r.get(0),
                )?;
                let count = cardinality(count)?;
                job.pass += 1;
                report.passes = job.pass;
                report.total += count;
                job.report = report;
                job.queue_count = count;
                job.cursor = 0;
                job.phase = if count == 0 {
                    "FIXPOINT_COMPLETE"
                } else {
                    "PASS_COMPLETE"
                }
                .into();
                save(&snapshot.scratch, &job)?;
                checkpoint("before_enumeration_commit")?;
                crate::store::release::settle(&snapshot.scratch.conn, tx.commit())?;
                checkpoint("after_enumeration_commit")?;
            }
            "PASS_COMPLETE" | "PROMOTING" => {
                let page = read_page(
                    &snapshot.scratch,
                    "owl_page_queue",
                    job.cursor,
                    page_size,
                    &job.timestamp,
                )?;
                if page.is_empty() {
                    let queued: i64 = snapshot.scratch.conn.query_row(
                        "SELECT count(*) FROM owl_page_queue",
                        [],
                        |r| r.get(0),
                    )?;
                    let total: i64 = snapshot.scratch.conn.query_row(
                        "SELECT count(*) FROM owl_page_all",
                        [],
                        |r| r.get(0),
                    )?;
                    if cardinality(queued)? != job.queue_count
                        || cardinality(total)? != job.report.total
                    {
                        return Err(invalid(
                            "OWL promoted queue cardinality differs from completed enumeration; incomplete",
                        ));
                    }
                    job.phase = "CAPTURED".into();
                    save(&snapshot.scratch, &job)?;
                    continue;
                }
                // Facts, cumulative output and cursor share one SQLite commit. No rules
                // run again until every page of this frozen pass has been promoted.
                let tx = snapshot.scratch.conn.unchecked_transaction()?;
                snapshot.scratch.conn.execute(
                    "INSERT INTO transactions(timestamp,source) VALUES(?1,'owl:materialize')",
                    [&job.timestamp],
                )?;
                let tx_id = snapshot.scratch.conn.last_insert_rowid();
                let graph = snapshot.scratch.intern(super::ROOT_INFERRED_GRAPH_IRI)?;
                for (_, d) in &page {
                    snapshot.scratch.conn.execute("INSERT OR IGNORE INTO facts(e,a,v,g,tx,valid_from,op) VALUES(?1,?2,?3,?4,?5,?6,1)",
                        params![d.entity,d.attribute,d.value.to_bytes(),graph,tx_id,d.valid_from])?;
                    snapshot.scratch.conn.execute(
                        "INSERT OR IGNORE INTO owl_page_all(e,a,v) VALUES(?1,?2,?3)",
                        params![d.entity, d.attribute, d.value.to_bytes()],
                    )?;
                    checkpoint("mid_promotion")?;
                }
                job.cursor = page.last().expect("nonempty page").0;
                job.phase = "PROMOTING".into();
                save(&snapshot.scratch, &job)?;
                checkpoint("before_promotion_commit")?;
                crate::store::release::settle(&snapshot.scratch.conn, tx.commit())?;
                checkpoint("after_promotion_commit")?;
            }
            _ => return Err(invalid("OWL durable job invalidated or failed")),
        }
    }
}

#[cfg(test)]
thread_local! { static FAIL_AT: std::cell::RefCell<Option<&'static str>> = const { std::cell::RefCell::new(None) }; }
#[cfg(test)]
pub(super) fn fail_at(point: &'static str) {
    FAIL_AT.with(|p| *p.borrow_mut() = Some(point));
}
pub(super) fn checkpoint(point: &str) -> Result<()> {
    #[cfg(test)]
    if FAIL_AT.with(|p| {
        if *p.borrow() == Some(point) {
            p.borrow_mut().take();
            true
        } else {
            false
        }
    }) {
        return Err(invalid("simulated crash"));
    }
    let _ = point;
    Ok(())
}
