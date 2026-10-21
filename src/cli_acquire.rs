//! `quipu acquire scope` — bounded physical scope of a direct-main acquisition
//! (aegis-ehf G1). The contract lives on `acquisition::scope`; this is argument
//! plumbing. The store is opened read-only, with no migration.

use crate::cli::flag_value;

const USAGE: &str = "usage: quipu acquire scope --source-store-id <id> --graph <iri> \
     [--graph <iri>]... [--tx <n>] --max-vm-steps <n> --deadline-ms <n> [--db <path>]";

fn number(args: &[String], flag: &str) -> Option<u64> {
    flag_value(args, flag).map(|s| {
        s.parse::<u64>().unwrap_or_else(|_| {
            eprintln!("{flag} must be a non-negative integer, got {s:?}");
            std::process::exit(1);
        })
    })
}

fn required(args: &[String], flag: &str) -> u64 {
    number(args, flag).unwrap_or_else(|| {
        eprintln!("{flag} is required: the run's budget is explicit\n{USAGE}");
        std::process::exit(1);
    })
}

/// `quipu acquire scope ...`: prints one measurement as JSON, or exits 1.
pub fn cmd_acquire(args: &[String], db_path: &str) {
    if args.get(2).map(String::as_str) != Some("scope") {
        eprintln!("{USAGE}");
        std::process::exit(1);
    }
    let Some(source_store_id) = flag_value(args, "--source-store-id") else {
        eprintln!("--source-store-id is required\n{USAGE}");
        std::process::exit(1);
    };
    let mut graphs = Vec::new();
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        if arg == "--graph"
            && let Some(iri) = it.next()
        {
            graphs.push(iri.clone());
        }
    }
    let tx = number(args, "--tx").map(|tx| {
        i64::try_from(tx).unwrap_or_else(|_| {
            eprintln!("--tx is out of range");
            std::process::exit(1);
        })
    });
    let request = quipu::acquisition::scope::Request {
        source_store_id: source_store_id.to_string(),
        graphs,
        tx,
        max_vm_steps: required(args, "--max-vm-steps"),
        deadline_ms: required(args, "--deadline-ms"),
    };
    let store = quipu::Store::open_read_only(db_path).unwrap_or_else(|e| {
        eprintln!("error opening store read-only: {e}");
        std::process::exit(1);
    });
    match quipu::acquisition::scope::measure(&store, &request) {
        Ok(measurement) => println!(
            "{}",
            serde_json::to_string_pretty(&measurement).expect("measurement serializes")
        ),
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    }
}
