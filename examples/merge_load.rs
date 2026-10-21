//! File-only driver for the shipped merge load paths; the oracle lives in Python.
use oxrdfio::RdfFormat;
use quipu::{Store, share::ShareOptions};
use serde_json::{Value, json};
use std::{fs, path::Path};

const TS: &str = "2026-10-20T00:00:00Z";

fn put(store: &mut Store, nt: &str) -> quipu::Result<()> {
    quipu::rdf::ingest_rdf(
        store,
        nt.as_bytes(),
        RdfFormat::NTriples,
        None,
        TS,
        None,
        Some("merge-load"),
    )?;
    Ok(())
}

fn text<'a>(v: &'a Value, key: &str) -> &'a str {
    v[key].as_str().expect("required string")
}

fn run(v: &Value) -> Result<Value, Box<dyn std::error::Error>> {
    let op = text(v, "op");
    if op == "materialize" {
        let r = quipu::share_delta::materialize(text(v, "parent"), text(v, "delta"))?;
        let out = Path::new(text(v, "out"));
        fs::create_dir(out)?;
        fs::write(
            out.join("manifest.json"),
            serde_json::to_vec_pretty(&r.manifest)?,
        )?;
        fs::write(out.join("export.nt"), r.export_ntriples)?;
        fs::write(out.join("shapes.ttl"), r.shapes_turtle)?;
        if let Some(q) = r.queries_turtle {
            fs::write(out.join("queries.ttl"), q)?;
        }
        return Ok(json!({"share_id": r.manifest.share_id}));
    }
    let mut store = Store::open(text(v, "db"))?;
    match op {
        "seed" => {
            let graph = store.overlay_create("urn:merge-load:catalogue", 0)?;
            quipu::rdf::ingest_rdf_to_graph(
                &mut store,
                include_bytes!("../tests/fixtures/share-catalogue.ttl").as_slice(),
                RdfFormat::Turtle,
                None,
                TS,
                None,
                Some("fixture-catalogue"),
                graph,
            )?;
            store.load_shapes("merge-load", &fs::read_to_string(text(v, "shapes"))?, TS)?;
            put(&mut store, &fs::read_to_string(text(v, "facts"))?)?;
            Ok(json!({"seeded": true, "store_id": store.store_id()?}))
        }
        "edit" => {
            // Only fixture-ledger slots are removed. The producer's own RDF
            // parser/writer executes additions; no SQL or mock store is used.
            for slot in v["remove"].as_array().expect("remove array") {
                let s = store.lookup(slot[0].as_str().unwrap())?;
                let p = store.lookup(slot[1].as_str().unwrap())?;
                if let (Some(s), Some(p)) = (s, p) {
                    store.retract_entity(s, Some(p), TS, None)?;
                }
            }
            put(&mut store, text(v, "insert"))?;
            Ok(json!({"edited": true}))
        }
        "share" => {
            let opts = ShareOptions {
                parent_share: v["parent_id"].as_str().map(str::to_string),
                ..Default::default()
            };
            Ok(serde_json::to_value(quipu::share::share(
                &store,
                text(v, "out"),
                &opts,
            )?)?)
        }
        "delta" => Ok(serde_json::to_value(quipu::share_delta::write_delta(
            &store,
            text(v, "parent"),
            text(v, "out"),
            &ShareOptions::default(),
        )?)?),
        "merge" => Ok(serde_json::to_value(quipu::share_merge::merge(
            &mut store,
            Path::new(text(v, "incoming")),
            TS,
            Some("load-fixture"),
        )?)?),
        "decisions" => {
            let incoming = Path::new(text(v, "incoming"));
            let mut file = quipu::share_merge_decisions::emit(&store, incoming)?;
            fs::write(text(v, "out"), serde_json::to_vec_pretty(&file)?)?;
            if v["apply"].as_bool() == Some(true) {
                for row in &mut file.rows {
                    row.decision = Some(quipu::share_merge_decisions::Decision::Choose {
                        choose: quipu::share_merge_decisions::Side::Ours,
                    });
                }
                let bytes = serde_json::to_vec(&file)?;
                return Ok(serde_json::to_value(quipu::share_merge_decisions::apply(
                    &mut store,
                    incoming,
                    &file,
                    &bytes,
                    "load-fixture",
                    TS,
                    None,
                )?)?);
            }
            Ok(serde_json::to_value(file)?)
        }
        "export" => {
            fs::write(
                text(v, "out"),
                quipu::rdf::export_rdf(&store, RdfFormat::NTriples)?,
            )?;
            Ok(json!({"exported": true}))
        }
        _ => Err(format!("unknown operation {op}").into()),
    }
}

fn main() {
    let path = std::env::args().nth(1).expect("merge_load request.json");
    let result = fs::read(&path)
        .map_err(|e| e.to_string())
        .and_then(|b| serde_json::from_slice(&b).map_err(|e| e.to_string()))
        .and_then(|v| run(&v).map_err(|e| e.to_string()));
    match result {
        Ok(v) => println!("{v}"),
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}
