//! Explicit local disk-store ownership operations; never invoked on ordinary open.
pub fn run(args: &[String], destination: &str) {
    if args.get(2).map(String::as_str) == Some("status") {
        match quipu::Store::open_read_only(destination).and_then(|s| s.ownership_state()) {
            Ok(state) => println!("{}", serde_json::json!({"ownership":state})),
            Err(e) => {
                eprintln!("{e}");
                std::process::exit(1);
            }
        }
        return;
    }
    let result = match args.get(2).map(String::as_str) {
        Some("adopt") => quipu::Store::adopt(destination),
        Some("restore") => {
            let mut words = args.iter().skip(3);
            let source = loop {
                let word = words.next().expect("syntax admission requires a source");
                if word == "--db" {
                    words.next();
                } else {
                    break word;
                }
            };
            quipu::Store::restore(source, destination)
        }
        _ => unreachable!("syntax admission validates store operation"),
    };
    match result {
        Ok(epoch) => println!("{}", serde_json::json!({"log_epoch":epoch, "owned":true})),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}
