//! `quipu validate`: dry-run SHACL validation of a data file against a shapes file.

pub fn cmd_validate(args: &[String]) {
    let shapes_path = args
        .windows(2)
        .find(|w| w[0] == "--shapes")
        .map(|w| w[1].as_str());
    let data_path = args
        .windows(2)
        .find(|w| w[0] == "--data")
        .map(|w| w[1].as_str());

    let format = args
        .windows(2)
        .find(|w| w[0] == "--format")
        .map_or("text", |w| w[1].as_str());

    let (Some(shapes_path), Some(data_path), "text" | "json") = (shapes_path, data_path, format)
    else {
        eprintln!(
            "usage: quipu validate --shapes <shapes.ttl> --data <data.ttl> [--format text|json]"
        );
        std::process::exit(1);
    };

    let shapes = match std::fs::read_to_string(shapes_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error reading shapes: {e}");
            std::process::exit(1);
        }
    };
    let data = match std::fs::read_to_string(data_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error reading data: {e}");
            std::process::exit(1);
        }
    };

    match quipu::validate_shapes(&shapes, &data) {
        // Every result, in engine order, with source shape, component and
        // value: a caller comparing two reports needs the full multiset, which
        // the text form drops (aegis-g3j). Exit status matches the text form.
        Ok(feedback) if format == "json" => {
            match serde_json::to_string(&feedback) {
                Ok(json) => println!("{json}"),
                Err(e) => {
                    eprintln!("validation error: {e}");
                    std::process::exit(1);
                }
            }
            if !feedback.conforms {
                std::process::exit(1);
            }
        }
        Ok(feedback) => {
            if feedback.conforms {
                println!("valid ({} warnings)", feedback.warnings);
            } else {
                println!(
                    "invalid: {} violation(s), {} warning(s)",
                    feedback.violations, feedback.warnings
                );
                for issue in &feedback.results {
                    println!(
                        "  [{:>9}] {} -- {}{}",
                        issue.severity,
                        issue.focus_node,
                        issue.message.as_deref().unwrap_or("constraint violated"),
                        issue
                            .path
                            .as_ref()
                            .map(|p| format!(" (path: {p})"))
                            .unwrap_or_default()
                    );
                }
                std::process::exit(1);
            }
        }
        Err(e) => {
            eprintln!("validation error: {e}");
            std::process::exit(1);
        }
    }
}
