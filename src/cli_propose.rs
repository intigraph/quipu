//! CLI command: `quipu propose` — schema evolution proposal management.

use crate::cli::chrono_now;

pub fn cmd_propose(args: &[String], db_path: &str) {
    let action = args.get(2).map_or("list", std::string::String::as_str);

    let mut store = crate::cli_open::open_store(db_path);

    match action {
        "submit" => cmd_propose_submit(args, &store),
        "plan" => crate::cli_propose_plan::cmd_plan(args, &store),
        "accept" => cmd_propose_accept(args, &mut store),
        "reject" => cmd_propose_reject(args, &store),
        _ => cmd_propose_list(args, &store),
    }
}

fn cmd_propose_submit(args: &[String], store: &quipu::Store) {
    let kind = match args.get(3) {
        Some(k) if !k.starts_with("--") => k.as_str(),
        _ => {
            eprintln!(
                "usage: quipu propose submit <kind> <target> <file.ttl> --proposer <id> \
                 [--rationale <text>] [--trigger <ref>] [--db <path>]"
            );
            std::process::exit(1);
        }
    };
    let target = match args.get(4) {
        Some(t) if !t.starts_with("--") => t.as_str(),
        _ => {
            eprintln!("usage: quipu propose submit <kind> <target> <file.ttl> ...");
            std::process::exit(1);
        }
    };
    let file_path = match args.get(5) {
        Some(p) if !p.starts_with("--") => p.as_str(),
        _ => {
            eprintln!("usage: quipu propose submit <kind> <target> <file.ttl> ...");
            std::process::exit(1);
        }
    };
    let diff = match std::fs::read_to_string(file_path) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("error reading {file_path}: {e}");
            std::process::exit(1);
        }
    };
    let proposer = match args.windows(2).find(|w| w[0] == "--proposer") {
        Some(w) => w[1].as_str(),
        None => {
            eprintln!("--proposer is required");
            std::process::exit(1);
        }
    };
    let rationale = args
        .windows(2)
        .find(|w| w[0] == "--rationale")
        .map(|w| w[1].as_str());
    let trigger_ref = args
        .windows(2)
        .find(|w| w[0] == "--trigger")
        .map(|w| w[1].as_str());

    let proposal_kind = match quipu::ProposalKind::from_json(kind) {
        Ok(k) => k,
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    };
    let now = chrono_now();
    match store.insert_proposal(&quipu::proposal::NewProposal {
        kind: &proposal_kind,
        target,
        diff: &diff,
        rationale,
        proposer,
        trigger_ref,
        created_at: &now,
    }) {
        Ok(id) => println!("proposal {id} created (status: pending)"),
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    }
}

/// Who decided, for `propose accept|reject` (aegis-il667q). An explicit
/// `--decided-by <id>` wins; otherwise the CLI names the invoking identity as
/// `cli:<QUIPU_AGENT or USER>`. It NEVER records an anonymous fixed string:
/// with no flag and no identity in the environment it refuses, matching the
/// MCP and REST paths where `decided_by` is required.
fn resolve_decided_by(
    args: &[String],
    env: impl Fn(&str) -> Option<String>,
) -> Result<String, &'static str> {
    // Decide PRESENCE first, then the value. A flag that is present but has no
    // usable value must refuse: falling through to the environment would record
    // a different actor than the one the caller tried to name.
    let mut explicit = None;
    for (i, arg) in args.iter().enumerate() {
        if arg == "--decided-by" {
            explicit = Some(args.get(i + 1).map_or("", String::as_str));
            break;
        }
        if let Some(v) = arg.strip_prefix("--decided-by=") {
            explicit = Some(v);
            break;
        }
    }
    if let Some(v) = explicit {
        let v = v.trim();
        if v.is_empty() || v.starts_with("--") {
            return Err("--decided-by needs a non-empty identity");
        }
        return Ok(v.to_string());
    }
    for key in ["QUIPU_AGENT", "USER"] {
        if let Some(v) = env(key)
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
        {
            return Ok(format!("cli:{v}"));
        }
    }
    Err("no approver identity: pass --decided-by <id> (QUIPU_AGENT and USER are unset)")
}

fn decided_by_or_exit(args: &[String]) -> String {
    resolve_decided_by(args, |k| std::env::var(k).ok()).unwrap_or_else(|e| {
        eprintln!("error: {e}");
        std::process::exit(1);
    })
}

fn cmd_propose_accept(args: &[String], store: &mut quipu::Store) {
    let id_str = match args.get(3) {
        Some(s) if !s.starts_with("--") => s.as_str(),
        _ => {
            eprintln!(
                "usage: quipu propose accept <id> [--note <text>] [--decided-by <id>] \
                 [--risk-threshold <0-100>] [--accept-risk --reason <why>] [--db <path>]"
            );
            std::process::exit(1);
        }
    };
    let id: i64 = id_str.parse().unwrap_or_else(|_| {
        eprintln!("invalid proposal id: {id_str}");
        std::process::exit(1);
    });
    let note = args
        .windows(2)
        .find(|w| w[0] == "--note")
        .map(|w| w[1].as_str());
    let decided_by = decided_by_or_exit(args);
    // aegis-493: the blast-radius gate runs before anything is written.
    let note = crate::cli_propose_plan::gate_or_exit(args, store, id, note);
    let now = chrono_now();
    match store.accept_proposal(id, &decided_by, &now, note.as_deref()) {
        Ok(p) => println!(
            "proposal {} accepted (target: {}, kind: {:?})",
            p.id, p.target, p.kind
        ),
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    }
}

fn cmd_propose_reject(args: &[String], store: &quipu::Store) {
    let id_str = match args.get(3) {
        Some(s) if !s.starts_with("--") => s.as_str(),
        _ => {
            eprintln!(
                "usage: quipu propose reject <id> --note <reason> [--decided-by <id>] [--db <path>]"
            );
            std::process::exit(1);
        }
    };
    let id: i64 = id_str.parse().unwrap_or_else(|_| {
        eprintln!("invalid proposal id: {id_str}");
        std::process::exit(1);
    });
    let note = match args.windows(2).find(|w| w[0] == "--note") {
        Some(w) => w[1].as_str(),
        None => {
            eprintln!("--note is required for rejection");
            std::process::exit(1);
        }
    };
    let decided_by = decided_by_or_exit(args);
    let now = chrono_now();
    match store.reject_proposal(id, &decided_by, &now, note) {
        Ok(p) => println!("proposal {} rejected (target: {})", p.id, p.target),
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    }
}

fn cmd_propose_list(args: &[String], store: &quipu::Store) {
    let status_filter = args
        .windows(2)
        .find(|w| w[0] == "--status")
        .map(|w| w[1].as_str());
    let status = status_filter
        .map(quipu::ProposalStatus::from_json)
        .transpose()
        .unwrap_or_else(|e| {
            eprintln!("error: {e}");
            std::process::exit(1);
        });
    match store.list_proposals(status.as_ref()) {
        Ok(proposals) => {
            if proposals.is_empty() {
                println!("no proposals found");
            } else {
                for p in &proposals {
                    println!(
                        "  #{} [{}] {:?} {} -- {} (by {})",
                        p.id,
                        match p.status {
                            quipu::ProposalStatus::Pending => "pending",
                            quipu::ProposalStatus::Accepted => "accepted",
                            quipu::ProposalStatus::Rejected => "rejected",
                        },
                        p.kind,
                        p.target,
                        p.rationale.as_deref().unwrap_or("(no rationale)"),
                        p.proposer,
                    );
                }
                println!("\n{} proposal(s)", proposals.len());
            }
        }
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod decided_by_tests {
    use super::resolve_decided_by;

    fn a(v: &[&str]) -> Vec<String> {
        v.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn explicit_flag_wins_over_environment() {
        let env = |k: &str| (k == "USER").then(|| "braino".to_string());
        let got = resolve_decided_by(
            &a(&[
                "quipu",
                "propose",
                "accept",
                "7",
                "--decided-by",
                "reviewer-1",
            ]),
            env,
        );
        assert_eq!(got.unwrap(), "reviewer-1");
    }

    #[test]
    fn falls_back_to_quipu_agent_then_user_never_cli_user() {
        let both = |k: &str| match k {
            "QUIPU_AGENT" => Some("muldoon".to_string()),
            "USER" => Some("braino".to_string()),
            _ => None,
        };
        assert_eq!(
            resolve_decided_by(&a(&["quipu", "propose", "accept", "7"]), both).unwrap(),
            "cli:muldoon"
        );
        let user_only = |k: &str| (k == "USER").then(|| "braino".to_string());
        assert_eq!(
            resolve_decided_by(
                &a(&["quipu", "propose", "reject", "7", "--note", "x"]),
                user_only
            )
            .unwrap(),
            "cli:braino"
        );
    }

    #[test]
    fn refuses_without_any_identity_or_with_an_empty_flag() {
        let none = |_: &str| None;
        assert!(resolve_decided_by(&a(&["quipu", "propose", "accept", "7"]), none).is_err());
        let blank = |_: &str| Some("  ".to_string());
        assert!(resolve_decided_by(&a(&["quipu", "propose", "accept", "7"]), blank).is_err());
        assert!(
            resolve_decided_by(
                &a(&["quipu", "propose", "accept", "7", "--decided-by", ""]),
                none
            )
            .is_err()
        );
        assert!(
            resolve_decided_by(
                &a(&["quipu", "propose", "accept", "7", "--decided-by", "--note"]),
                none
            )
            .is_err()
        );
    }

    #[test]
    fn explicit_flag_without_a_value_refuses_even_with_a_fallback_identity() {
        // wu's falsifier on #608: a TRAILING --decided-by had no two-token window,
        // so it was read as absent and the decision was recorded as cli:<USER>.
        let env = |k: &str| (k == "USER").then(|| "review-user".to_string());
        for verb in ["accept", "reject"] {
            for tail in [
                &["--decided-by"][..],
                &["--decided-by", ""][..],
                &["--decided-by", "   "][..],
                &["--decided-by", "--note", "x"][..],
                &["--decided-by="][..],
            ] {
                let mut argv = vec!["quipu", "propose", verb, "7"];
                argv.extend_from_slice(tail);
                assert!(
                    resolve_decided_by(&a(&argv), env).is_err(),
                    "{argv:?} must refuse, not fall back to cli:review-user"
                );
            }
        }
        // Positive controls under the same environment.
        assert_eq!(
            resolve_decided_by(
                &a(&["quipu", "propose", "reject", "7", "--decided-by=ian"]),
                env
            )
            .unwrap(),
            "ian"
        );
        assert_eq!(
            resolve_decided_by(&a(&["quipu", "propose", "reject", "7"]), env).unwrap(),
            "cli:review-user"
        );
    }
}
