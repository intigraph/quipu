//! Explicitly provisioned, transaction-maintained substring candidates.
//! Searches never create or backfill this optional current-only derived index.
//! Temporal queries retain the ordinary exact fact evaluator.
use rusqlite::{OptionalExtension, functions::FunctionFlags, params};
use crate::{Error, Result, Store, Value};

const FTS: &str = "CREATE VIRTUAL TABLE substring_fts USING fts5(scope,text,exact_text UNINDEXED,tokenize='trigram case_sensitive 1')";
const SCHEMA: &str = r#"
CREATE VIRTUAL TABLE substring_fts USING fts5(scope,text,exact_text UNINDEXED,tokenize='trigram case_sensitive 1');
CREATE TABLE substring_progress (
 id INTEGER PRIMARY KEY CHECK(id=1), cursor INTEGER NOT NULL,
 highwater INTEGER NOT NULL, complete INTEGER NOT NULL, version INTEGER NOT NULL
);
INSERT INTO substring_progress SELECT 1,0,h,h=0,5 FROM
 (SELECT coalesce(max(rowid),0) AS h FROM facts);
CREATE TABLE substring_facts (
 fact_id INTEGER PRIMARY KEY, e INTEGER NOT NULL, a INTEGER NOT NULL, g INTEGER NOT NULL,
 plain INTEGER NOT NULL, value_hash BLOB NOT NULL, needs_exact INTEGER NOT NULL
);
CREATE VIEW substring_source AS
 SELECT rowid AS fact_id, printf('g=%lld;p=%lld;',g,a) AS scope,
 quipu_substring_text(v) AS text, quipu_substring_exact(v) AS exact_text,
 e,a,g,quipu_substring_plain(v) AS plain,
 quipu_substring_hash(v) AS value_hash,
 quipu_substring_exact(v) IS NOT NULL AS needs_exact
 FROM facts NOT INDEXED WHERE op=1 AND valid_to IS NULL AND quipu_substring_text(v)<>'';
CREATE TRIGGER substring_insert AFTER INSERT ON facts BEGIN
 INSERT INTO substring_fts(rowid,scope,text,exact_text)
 SELECT fact_id,scope,text,exact_text FROM substring_source WHERE fact_id=NEW.rowid;
 INSERT INTO substring_facts
 SELECT fact_id,e,a,g,plain,value_hash,needs_exact FROM substring_source WHERE fact_id=NEW.rowid;
END;
CREATE TRIGGER substring_delete AFTER DELETE ON facts BEGIN
 DELETE FROM substring_fts WHERE rowid=OLD.rowid;
 DELETE FROM substring_facts WHERE fact_id=OLD.rowid;
END;
CREATE TRIGGER substring_update AFTER UPDATE ON facts BEGIN
 DELETE FROM substring_fts WHERE rowid=OLD.rowid;
 DELETE FROM substring_facts WHERE fact_id=OLD.rowid;
 INSERT INTO substring_fts(rowid,scope,text,exact_text)
 SELECT fact_id,scope,text,exact_text FROM substring_source WHERE fact_id=NEW.rowid;
 INSERT INTO substring_facts
 SELECT fact_id,e,a,g,plain,value_hash,needs_exact FROM substring_source WHERE fact_id=NEW.rowid;
END;
"#;

fn decoded(ctx: &rusqlite::functions::Context<'_>) -> rusqlite::Result<Value> {
    let bytes:Vec<u8>=ctx.get(0)?;
    Value::from_bytes(&bytes).map_err(|e|rusqlite::Error::UserFunctionError(Box::new(e)))
}
fn text(value:&Value)->Option<&str> {
    match value {Value::Str(s)|Value::Lang{lexical:s,..}|Value::Typed{lexical:s,..}=>Some(s),_=>None}
}
pub(crate) fn register(conn: &rusqlite::Connection) -> rusqlite::Result<()> {
    let flags=FunctionFlags::SQLITE_UTF8|FunctionFlags::SQLITE_DETERMINISTIC|FunctionFlags::SQLITE_INNOCUOUS;
    for name in ["quipu_substring_text","quipu_substring_exact"] {
        conn.create_scalar_function(name,1,flags,move|ctx| {
            let value=decoded(ctx)?;let lower=text(&value).unwrap_or("").to_lowercase();
            // Normal strings already have their exact lowercase representation
            // in `text`. Keep a second copy only when NUL normalization differs.
            Ok(if name=="quipu_substring_text" {Some(lower.replace('\0'," "))}
                else {lower.contains('\0').then_some(lower)})
        })?;
    }
    conn.create_scalar_function("quipu_substring_plain",1,flags,|ctx| {
        let value=decoded(ctx)?;
        Ok(text(&value).is_some_and(|s|value.term_key()==Value::Str(s.to_owned()).term_key()))
    })?;
    // This compact prefix only groups candidates. Projection verifies actual
    // canonical terms whenever a subject has repeated fingerprints.
    conn.create_scalar_function("quipu_substring_hash",1,flags,|ctx| {
        let value=decoded(ctx)?;
        Ok(ring::digest::digest(&ring::digest::SHA256,&value.term_key()).as_ref()[..8].to_vec())
    })?;
    Ok(())
}

/// Durable bounded-backfill state; completion includes transactional future writes.
#[derive(Debug, serde::Serialize)]
pub struct SubstringProgress {
    /// Last backfilled physical assertion row.
    pub cursor: i64,
    /// Finite high-water mark captured at explicit initialization.
    pub highwater: i64,
    /// Whether every assertion through that mark has been visited.
    pub complete: bool,
}
fn definition(kind:&str,name:&str)->Option<&'static str> {
    let at=SCHEMA.find(&format!("CREATE {kind} {name} "))?;
    let tail=&SCHEMA[at..];
    let end=if kind=="TRIGGER" {tail.find("\nEND;")?+5}else{tail.find(";\n")?};
    Some(tail[..end].trim().trim_end_matches(';'))
}
fn atomic(conn:&rusqlite::Connection,body:impl FnOnce()->Result<()>)->Result<()> {
    conn.execute_batch("SAVEPOINT substring_change")?;
    let result=body();
    if result.is_err(){conn.execute_batch("ROLLBACK TO substring_change")?;}
    conn.execute_batch("RELEASE substring_change")?;
    result
}
impl Store {
    /// Observe supported derived-index state without creating it or scanning facts.
    pub fn substring_progress(&self) -> Result<Option<SubstringProgress>> {
        let exists:bool=self.conn.query_row("SELECT EXISTS(SELECT 1 FROM main.sqlite_master WHERE name='substring_progress' AND type='table')",[],|r|r.get(0))?;
        if !exists{return Ok(None);}
        Ok(self.conn.query_row("SELECT cursor,highwater,complete FROM main.substring_progress WHERE id=1 AND version=5",[],|r|Ok(SubstringProgress{cursor:r.get(0)?,highwater:r.get(1)?,complete:r.get(2)?})).optional()?)
    }
    /// A complete index requires the exact tokenizer, projection and maintenance DDL.
    pub fn substring_index_ready(&self) -> Result<bool> {
        if self.has_attachments()||!self.substring_progress()?.is_some_and(|p|p.complete&&p.cursor>=p.highwater){return Ok(false);}
        let fts:Option<String>=self.conn.query_row("SELECT sql FROM main.sqlite_master WHERE name='substring_fts' AND type='table'",[],|r|r.get(0)).optional()?;
        if fts.as_deref().map(str::trim)!=Some(FTS){return Ok(false);}
        for (kind,name) in [("TABLE","substring_progress"),("TABLE","substring_facts"),("VIEW","substring_source"),("TRIGGER","substring_insert"),("TRIGGER","substring_delete"),("TRIGGER","substring_update")] {
            let actual:Option<String>=self.conn.query_row("SELECT sql FROM main.sqlite_master WHERE name=?1 AND type=?2",params![name,kind.to_lowercase()],|r|r.get(0)).optional()?;
            if actual.as_deref().map(str::trim)!=definition(kind,name){return Ok(false);}
        }
        Ok(true)
    }
    /// Explicitly install empty derived schema and writer triggers, atomically.
    /// Unsupported old layouts require an explicit drop/rebuild; never auto-upgrade.
    pub fn initialize_substring_index(&self) -> Result<()> {
        if self.substring_progress()?.is_some(){return Ok(());}
        atomic(&self.conn,||{self.conn.execute_batch(SCHEMA)?;Ok(())})
    }
    /// Exactly one explicit bounded transaction; never called by query evaluation.
    pub fn backfill_substring_batch(&self,batch_size:usize)->Result<SubstringProgress> {
        if !(1..=10_000).contains(&batch_size){return Err(Error::InvalidValue("substring batch size must be 1..10000".into()));}
        self.initialize_substring_index()?;
        atomic(&self.conn,|| {
            let state=self.substring_progress()?.ok_or_else(||Error::Store("substring state missing".into()))?;
            let end:i64=self.conn.query_row("SELECT coalesce(max(rowid),?2) FROM (SELECT rowid FROM facts WHERE rowid>?1 AND rowid<=?2 ORDER BY rowid LIMIT ?3)",params![state.cursor,state.highwater,batch_size as i64],|r|r.get(0))?;
            self.conn.execute("DELETE FROM substring_fts WHERE rowid>?1 AND rowid<=?2",params![state.cursor,end])?;
            self.conn.execute("DELETE FROM substring_facts WHERE fact_id>?1 AND fact_id<=?2",params![state.cursor,end])?;
            self.conn.execute("INSERT INTO substring_fts(rowid,scope,text,exact_text) SELECT fact_id,scope,text,exact_text FROM substring_source WHERE fact_id>?1 AND fact_id<=?2",params![state.cursor,end])?;
            self.conn.execute("INSERT INTO substring_facts SELECT fact_id,e,a,g,plain,value_hash,needs_exact FROM substring_source WHERE fact_id>?1 AND fact_id<=?2",params![state.cursor,end])?;
            self.conn.execute("UPDATE substring_progress SET cursor=?1,complete=(?1>=highwater) WHERE id=1 AND version=5",[end])?;
            Ok(())
        })?;
        self.substring_progress()?.ok_or_else(||Error::Store("substring state missing".into()))
    }
    /// Remove only derived data and its triggers; preserve the fact log.
    pub fn drop_substring_index(&self)->Result<()> {
        atomic(&self.conn,||{self.conn.execute_batch("DROP TRIGGER IF EXISTS substring_insert;DROP TRIGGER IF EXISTS substring_update;DROP TRIGGER IF EXISTS substring_delete;DROP VIEW IF EXISTS substring_source;DROP TABLE IF EXISTS substring_fts;DROP TABLE IF EXISTS substring_facts;DROP TABLE IF EXISTS substring_progress")?;Ok(())})
    }
}
#[cfg(test)]mod tests;
