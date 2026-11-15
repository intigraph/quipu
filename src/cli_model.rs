//! Explicit model provisioning; ordinary opens stay offline.
pub fn run(args: &[String], db: &str) {
    if args.get(2).map(String::as_str) == Some("install") {
        return install(db);
    }
    let number = |flag, default| -> i64 {
        crate::cli::flag_value(args, flag)
            .unwrap_or(default)
            .parse()
            .unwrap_or_else(|_| {
                eprintln!("{flag} must be an integer");
                std::process::exit(1);
            })
    };
    let after = number("--after", "0");
    let limit = number("--batch-size", "32");
    if after < 0 || !(1..=256).contains(&limit) {
        eprintln!("model backfill requires after >= 0 and batch-size in 1..=256");
        std::process::exit(1);
    }
    let store = crate::cli_open::open_store(db);
    match quipu::model_backfill::batch(
        &store,
        after,
        usize::try_from(limit).expect("validated batch size"),
    ) {
        Ok(progress) => println!(
            "{}",
            serde_json::json!({"scope":"ROOT subjects", "progress":progress})
        ),
        Err(error) => {
            eprintln!("model backfill: {error}");
            std::process::exit(1);
        }
    }
}

pub fn install(db: &str) {
    let result = quipu::model_assets::default_directory().and_then(|directory| {
        quipu::model_assets::install(&directory)?;
        // Installing for a future store remains asset-only. For an existing
        // local store this explicit command also prepares a bounded prefix;
        // ordinary opens still never backfill or download anything.
        let preparation = if std::path::Path::new(db).is_file() {
            let store = crate::cli_open::open_store(db);
            Some(quipu::model_backfill::prepare(&store, 32)?)
        } else {
            None
        };
        let next = match &preparation {
            Some(progress) if !progress.complete => format!(
                "continue preparation: quipu model backfill --after {} --batch-size 32 --db <same database>", progress.next_cursor
            ),
            Some(progress) if progress.stale_skipped > 0 =>
                "concurrent changes skipped; rerun quipu model install to revisit them".into(),
            _ => "check readiness with quipu doctor search; new opens discover this model automatically".into(),
        };
        Ok(serde_json::json!({
            "installed": true,
            "directory": directory,
            "revision": quipu::model_assets::REVISION,
            "dimension": 384,
            "scope": "ROOT subjects in an existing local SQLite database",
            "preparation": preparation,
            "next": next
        }))
    });
    match result {
        Ok(value) => println!("{value}"),
        Err(error) => {
            eprintln!("model install: {error}");
            std::process::exit(1);
        }
    }
}
