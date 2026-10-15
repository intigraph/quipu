//! PRIVATE diagnostic only; never a production command or public search API.
use rusqlite::{Connection, params};
use std::time::{Duration, Instant};
fn sql(mode: &str) -> &'static str {
    match mode {
        "postings" => "SELECT rowid FROM substring_fts WHERE substring_fts MATCH ?1 LIMIT 100001",
        "facts" => "SELECT e FROM facts NOT INDEXED WHERE a=?2 AND g=?3 AND op=1 AND valid_to IS NULL AND rowid IN (SELECT rowid FROM substring_fts WHERE substring_fts MATCH ?1) LIMIT 100001",
        "values" => "SELECT e,v FROM facts NOT INDEXED WHERE a=?2 AND g=?3 AND op=1 AND valid_to IS NULL AND rowid IN (SELECT rowid FROM substring_fts WHERE substring_fts MATCH ?1) LIMIT 100001",
        _ => panic!("unknown diagnostic mode"),
    }
}
fn run(conn: &Connection, mode: &str, phrase: &str, graph: i64, predicate: i64) -> rusqlite::Result<(usize, usize)> {
    let started = Instant::now();
    let mut stmt = conn.prepare(sql(mode))?;
    let mut rows = if mode == "postings" { stmt.query(params![phrase])? } else { stmt.query(params![phrase,predicate,graph])? };
    let mut count=0;let mut bytes=0;
    while let Some(row)=rows.next()? {
        std::hint::black_box(row.get::<_,i64>(0)?);
        if mode=="values" { let value:Vec<u8>=row.get(1)?;bytes+=value.len();std::hint::black_box(value); }
        count+=1;assert!(count<=100000,"diagnostic row ceiling");assert!(started.elapsed()<Duration::from_secs(45),"diagnostic deadline");
    }
    Ok((count,bytes))
}
fn main() {
    let args:Vec<_>=std::env::args().collect();assert_eq!(args.len(),3);
    let db=&args[1];let mode=&args[2];sql(mode);
    let wal=std::path::PathBuf::from(format!("{db}-wal"));assert!(!wal.exists()||wal.metadata().unwrap().len()==0,"nonempty WAL refused");
    let conn=Connection::open_with_flags(format!("file:{db}?immutable=1"),rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY|rusqlite::OpenFlags::SQLITE_OPEN_URI).unwrap();
    conn.execute_batch("PRAGMA query_only=ON;PRAGMA mmap_size=268435456;PRAGMA cache_size=-32000;").unwrap();
    let graph:i64=conn.query_row("SELECT id FROM terms WHERE iri=?1",["https://seeds.local/project/aegis"],|r|r.get(0)).unwrap();
    let predicate:i64=conn.query_row("SELECT id FROM terms WHERE iri=?1",["https://schema.org/name"],|r|r.get(0)).unwrap();
    let phrase=format!("scope:\"g={graph};p={predicate};\" AND text:\"quipu\"");
    std::thread::sleep(Duration::from_secs(2));
    let started=Instant::now();let (count,bytes)=run(&conn,mode,&phrase,graph,predicate).unwrap();assert!(count>0,"populated positive control");
    println!("{}",serde_json::json!({"mode":mode,"rows":count,"value_bytes":bytes,"elapsed":started.elapsed().as_secs_f64()}));
}
#[cfg(test)]mod tests {
 use super::*;
 #[test]fn stages_measure_postings_then_scoped_facts_then_values() {
  let c=Connection::open_in_memory().unwrap();c.execute_batch("CREATE TABLE facts(e INTEGER,a INTEGER,g INTEGER,v BLOB,op INTEGER,valid_to TEXT);CREATE VIRTUAL TABLE substring_fts USING fts5(scope,text,tokenize='trigram case_sensitive 1');INSERT INTO facts(rowid,e,a,g,v,op) VALUES(1,1,3,2,x'010203',1),(2,2,3,2,x'0405',1),(3,3,4,2,x'06',1);INSERT INTO substring_fts(rowid,scope,text) VALUES(1,'g=2;p=3;','quipu'),(2,'g=2;p=3;','unrelated'),(3,'g=2;p=4;','quipu');").unwrap();
  let phrase="scope:\"g=2;p=3;\" AND text:\"quipu\"";
  assert_eq!(run(&c,"postings",phrase,2,3).unwrap(),(1,0));assert_eq!(run(&c,"facts",phrase,2,3).unwrap(),(1,0));assert_eq!(run(&c,"values",phrase,2,3).unwrap(),(1,3));
 }
}
