use oxrdfio::RdfFormat;
use quipu::Store;
use quipu::share::{ShareDestination, ShareOptions};
fn main() -> quipu::Result<()> {
    let db = std::env::args().nth(1).expect("private database path");
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&db)
        .map_err(|e| {
            quipu::Error::InvalidValue(format!("fixture requires a new private database path: {e}"))
        })?;
    let mut source = Store::open_in_memory()?;
    quipu::rdf::ingest_rdf(
        &mut source,
        &b"<urn:a> <urn:q> \"secret\" ; <urn:r> \"keep\" ."[..],
        RdfFormat::Turtle,
        None,
        "2026-10-16T01:00:00Z",
        None,
        None,
    )?;
    source.load_shapes("fixture", "", "2026-10-16T01:00:00Z")?;
    let dir = tempfile::tempdir().unwrap();
    let pack = dir.path().join("pack");
    quipu::share::share(
        &source,
        pack.to_str().unwrap(),
        &ShareOptions {
            destination: ShareDestination::Internal,
            ..Default::default()
        },
    )?;
    let mut request = quipu::share_transport::read_local(pack.to_str().unwrap())?;
    request.destination = ShareDestination::Internal;
    let mut store = Store::open(&db)?;
    let first = quipu::share_current::compose_current(
        &mut store,
        "scbrown/demo",
        &["packs/main".into()],
        std::slice::from_ref(&request),
        None,
        "2026-10-16T02:00:00Z",
        None,
    )?;
    let graph = store
        .lookup(&first.current.unwrap().members[0].graph)?
        .unwrap();
    let datum = quipu::store::Datum {
        entity: store.intern("urn:a")?,
        attribute: store.intern("urn:q")?,
        value: quipu::types::Value::Str("secret".into()),
        valid_from: "2026-10-16T03:00:00Z".into(),
        valid_to: None,
        op: quipu::types::Op::Retract,
    };
    store.transact_to_graph(
        &[datum],
        "2026-10-16T03:00:00Z",
        Some("fixture"),
        None,
        graph,
    )?;
    quipu::share_current::compose_current(
        &mut store,
        "scbrown/demo",
        &["packs/main".into()],
        &[request],
        None,
        "2026-10-16T04:00:00Z",
        None,
    )?;
    println!("Private fixture: one carried-forward tombstone; unrelated keep fact present");
    Ok(())
}
