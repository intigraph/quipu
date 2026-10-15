//! Private one-shot projection admission diagnostic; not a public interface.
use std::time::Instant;
fn main() -> Result<(),Box<dyn std::error::Error>> {
 let args:Vec<_>=std::env::args().collect();assert_eq!(args.len(),2);
 let db=&args[1];let wal=std::path::PathBuf::from(format!("{db}-wal"));assert!(!wal.exists()||wal.metadata()?.len()==0,"nonempty WAL");
 let store=quipu::Store::open_immutable(db)?;
 let ready=store.substring_index_ready()?;
 println!("{}",serde_json::json!({"ready":ready,"attachments":store.has_attachments()}));
 assert!(ready,"complete compact index must admit");
 let q="SELECT ?s WHERE { GRAPH <https://seeds.local/project/aegis> { { SELECT ?s WHERE { ?s <https://schema.org/name> ?text . FILTER(isLiteral(?text) && sameTerm(?text,STR(?text)) && CONTAINS(LCASE(STR(?text)),\"quipu\")) } } } }";
 let start=Instant::now();let result=quipu::sparql::query(&store,q)?;
 println!("{}",serde_json::json!({"rows":result.rows().len(),"elapsed":start.elapsed().as_secs_f64()}));
 assert!(!result.rows().is_empty(),"populated target control");Ok(())
}
