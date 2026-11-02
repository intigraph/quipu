//! `quipu propose plan <id>` and the accept-time risk gate (aegis-493).
//!
//! Exit status of `plan`: 0 pass, 1 fail (risk over the threshold),
//! 2 not determined (the plan could not be computed, or could not be computed
//! honestly). It never writes.

use quipu::proposal_plan::{self, ProposalPlan};

use crate::cli::flag_value;

/// `--risk-threshold <0-100>`, else the deployment default.
fn threshold_or_exit(args: &[String], usage_code: i32) -> u8 {
    flag_value(args, "--risk-threshold").map_or_else(proposal_plan::default_threshold, |v| {
        v.parse::<u8>()
            .ok()
            .filter(|t| *t <= 100)
            .unwrap_or_else(|| {
                eprintln!("error: --risk-threshold must be an integer 0-100, got {v}");
                std::process::exit(usage_code);
            })
    })
}

fn id_or_exit(args: &[String], usage: &str, code: i32) -> i64 {
    let Some(raw) = args.get(3).filter(|s| !s.starts_with("--")) else {
        eprintln!("{usage}");
        std::process::exit(code);
    };
    raw.parse().unwrap_or_else(|_| {
        eprintln!("invalid proposal id: {raw}");
        std::process::exit(code);
    })
}

fn plan_or_exit(store: &quipu::Store, id: i64, threshold: u8, code: i32) -> ProposalPlan {
    proposal_plan::plan(store, id, threshold).unwrap_or_else(|e| {
        eprintln!("error: {e}");
        std::process::exit(code);
    })
}

pub fn cmd_plan(args: &[String], store: &quipu::Store) {
    const USAGE: &str = "usage: quipu propose plan <id> [--format text|json] [--risk-threshold <0-100>] [--db <path>]";
    let id = id_or_exit(args, USAGE, 2);
    let format = flag_value(args, "--format").unwrap_or("text");
    if !matches!(format, "text" | "json") {
        eprintln!("{USAGE}");
        std::process::exit(2);
    }
    let threshold = threshold_or_exit(args, 2);
    let plan = plan_or_exit(store, id, threshold, 2);
    if format == "json" {
        match serde_json::to_string_pretty(&plan) {
            Ok(json) => println!("{json}"),
            Err(e) => {
                eprintln!("error: {e}");
                std::process::exit(2);
            }
        }
    } else {
        print!("{}", proposal_plan::render_text(&plan));
    }
    std::process::exit(plan.risk.verdict.exit_code());
}

/// Plan proposal `id` and refuse (exit 1) unless it passes or the caller gave
/// `--accept-risk --reason <why>`. Returns the decision note to record.
pub fn gate_or_exit(
    args: &[String],
    store: &quipu::Store,
    id: i64,
    note: Option<&str>,
) -> Option<String> {
    let threshold = threshold_or_exit(args, 1);
    let accept_risk = args.iter().any(|a| a == "--accept-risk");
    let reason = flag_value(args, "--reason");
    if accept_risk && reason.is_none_or(|r| r.trim().is_empty() || r.starts_with("--")) {
        eprintln!("error: --accept-risk needs --reason <why>");
        std::process::exit(1);
    }
    let plan = match proposal_plan::plan_for_accept(store, id, threshold) {
        Ok(Some(plan)) => plan,
        // Not pending or absent: the accept itself reports it, as before.
        Ok(None) => return note.map(str::to_owned),
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    };
    let reason = if accept_risk { reason } else { None };
    match proposal_plan::gate(&plan, reason, note) {
        Ok(note) => note,
        Err(e) => {
            eprint!("{}", proposal_plan::render_text(&plan));
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    }
}
