//! Explicit local-repository CLI for the shared versioned-file core.
//! This exports a file-data envelope, never overwrites a native harness root.

use quipu::{Error, Result, Store, memory_files as memory};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use std::{fs::File, io::Read};

const HELP: &str = "qp memory read|export <owner.json> --graph IRI --db PATH\n\
qp memory receipt <request.json> --graph IRI --db PATH\n\
qp memory commit <request.json> --graph IRI --db PATH --local-authority\n\
Explicit local filesystem authority; actor fields are not remote authentication.\n\
export prints a bounded JSON file-data envelope, not a native-file projection.\n";

fn bad(message: &str) -> Error {
    Error::InvalidValue(message.into())
}

fn input<T: DeserializeOwned>(path: &str) -> Result<T> {
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|_| bad("memory input unreadable"))?
        .take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| bad("memory input unreadable"))?;
    if bytes.len() > 1024 * 1024 {
        return Err(bad("memory input exceeds limit"));
    }
    serde_json::from_slice(&bytes).map_err(|_| bad("invalid memory input JSON"))
}

fn execute(args: &[String]) -> Result<Value> {
    let action = args.get(2).ok_or_else(|| bad("memory action required"))?;
    if !["read", "export", "receipt", "commit"].contains(&action.as_str()) {
        return Err(bad("unknown memory action"));
    }
    let path = args
        .get(3)
        .filter(|s| !s.starts_with("--"))
        .ok_or_else(|| bad("memory input file required"))?;
    let (mut db, mut graph, mut local) = (None, None, false);
    let mut i = 4;
    while i < args.len() {
        match args[i].as_str() {
            "--db" | "--graph" => {
                let value = args
                    .get(i + 1)
                    .filter(|s| !s.is_empty() && !s.starts_with("--"))
                    .ok_or_else(|| bad("memory option value required"))?;
                let target = if args[i] == "--db" {
                    &mut db
                } else {
                    &mut graph
                };
                if target.replace(value).is_some() {
                    return Err(bad("duplicate memory option"));
                }
                i += 2;
            }
            "--local-authority" if !local => {
                local = true;
                i += 1;
            }
            _ => return Err(bad("unrecognized memory option")),
        }
    }
    let db = db.ok_or_else(|| bad("memory requires explicit --db"))?;
    let graph = graph.ok_or_else(|| bad("memory requires explicit --graph"))?;
    if oxrdf::NamedNode::new(graph.as_str()).is_err() {
        return Err(bad("invalid memory graph IRI"));
    }
    if action == "commit" && !local {
        return Err(bad("local memory commit requires --local-authority"));
    }
    // Validate input before opening or creating a repository.
    let (owner, request) = if action == "commit" || action == "receipt" {
        let request: memory::Commit = input(path)?;
        (request.owner.clone(), Some(request))
    } else {
        (input::<memory::Owner>(path)?, None)
    };
    let mut store = Store::open(db)?;
    store.set_base_ns(crate::cli_open::config().base_ns.clone());
    let graph_id = if graph == quipu::schema::ROOT_GRAPH_IRI {
        0
    } else {
        store
            .registered_graph_id(graph)?
            .ok_or_else(|| bad("memory graph is not registered"))?
    };
    if action == "receipt" {
        let receipt = memory::receipt(
            &store,
            graph_id,
            request.as_ref().expect("parsed request"),
            memory::Authority::Local,
        )?;
        return Ok(json!({"authority":"local_filesystem", "receipt":receipt}));
    }
    if let Some(request) = request {
        let receipt = memory::commit(
            &mut store,
            graph_id,
            &request,
            memory::Authority::Local,
            &quipu::time::now_iso(),
        )?;
        memory::revision(&store, graph_id, &receipt)?;
        return Ok(json!({"authority":"local_filesystem", "receipt":receipt}));
    }
    let snapshot = memory::read(&store, graph_id, &owner)?;
    if action == "read" {
        return Ok(json!({"coverage":"this_repository_owner_records", "snapshot":snapshot}));
    }
    serde_json::to_value(memory::export(&store, graph_id, &owner)?)
        .map_err(|_| bad("invalid memory export"))
}

pub fn command(args: &[String]) {
    if args.iter().any(|s| s == "--help" || s == "-h") {
        print!("{HELP}");
        return;
    }
    match execute(args) {
        Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
        Err(error) => {
            eprintln!("memory: {error}");
            std::process::exit(1);
        }
    }
}
