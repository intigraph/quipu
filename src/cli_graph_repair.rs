//! Explicit registry maintenance; default invocation only counts.
pub fn run(args: &[String], db: &str) {
    let execute = args.iter().any(|arg| arg == "--execute");
    if execute && args.iter().any(|arg| arg == "--dry-run") {
        eprintln!("error: choose --dry-run or --execute");
        std::process::exit(1);
    }
    let result = (|| -> quipu::Result<serde_json::Value> {
        let store = if execute {
            quipu::Store::open(db)?
        } else {
            quipu::Store::open_read_only(db)?
        };
        let before = store.unregistered_data_graph_count()?;
        if execute {
            store.repair_data_graph_registry()?;
        }
        Ok(
            serde_json::json!({"dry_run": !execute, "unregistered_graphs": before,
            "remaining": store.unregistered_data_graph_count()?}),
        )
    })();
    match result {
        Ok(value) => println!("{value}"),
        Err(error) => {
            eprintln!("error: {error}");
            std::process::exit(1);
        }
    }
}
