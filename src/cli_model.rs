//! Explicit model provisioning; ordinary opens stay offline.
pub fn run(args: &[String], db: &str) {
    if args.get(2).map(String::as_str) == Some("install") {
        return install();
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

pub fn install() {
    let result = quipu::model_assets::default_directory().and_then(|directory| {
        quipu::model_assets::install(&directory)?;
        Ok(serde_json::json!({
            "installed": true,
            "directory": directory,
            "revision": quipu::model_assets::REVISION,
            "dimension": 384,
            "next": "new CLI/server opens discover this model automatically; run quipu model backfill --batch-size 32 for existing ROOT subjects, resuming with --after next_cursor"
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
