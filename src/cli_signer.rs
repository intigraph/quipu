//! Operator-only signer registration over a local store file. Enrollment never
//! protects a graph or enables writes; each is a separate explicit operation.
use quipu::store::attested_write::Signer;

fn fail(error: impl std::fmt::Display) -> ! {
    eprintln!("signer: {error}");
    std::process::exit(1)
}

fn values<'a>(args: &'a [String], name: &str) -> Vec<&'a str> {
    args[3..]
        .as_chunks::<2>()
        .0
        .iter()
        .filter(|p| p[0] == name)
        .map(|p| p[1].as_str())
        .collect()
}

fn required(args: &[String], name: &str) -> String {
    let values = values(args, name);
    if values.len() != 1 {
        eprintln!("signer enroll requires exactly one {name}");
        std::process::exit(2);
    }
    values[0].to_owned()
}

pub fn cmd_signer(raw_args: &[String], db_path: &str) {
    // The global database option is legal before or after the subcommand.
    // Consume option values literally; a credential value named --db is data.
    let mut normalized = raw_args[..2].to_vec();
    let mut i = 2;
    while i < raw_args.len() {
        if raw_args[i].starts_with("--") {
            if raw_args[i] != "--db" {
                normalized.extend_from_slice(&raw_args[i..i + 2]);
            }
            i += 2;
        } else {
            normalized.push(raw_args[i].clone());
            i += 1;
        }
    }
    let args = normalized.as_slice();
    match args.get(2).map(String::as_str) {
        Some("enroll") => enroll(args, db_path),
        Some("status") => {
            let store = crate::cli_open::open_store(db_path);
            let enabled = store.attest_enabled().unwrap_or_else(|e| fail(e));
            let signers = store.attest_signers().unwrap_or_else(|e| fail(e));
            let signers: Vec<_> = signers
                .into_iter()
                .map(|s| {
                    serde_json::json!({
                        "id":s.signer.id, "scheme":s.signer.scheme, "origin":s.signer.origin,
                        "rp_id":s.signer.rp_id, "actor":s.signer.actor, "graphs":s.signer.graphs,
                        "expires_at":s.signer.expires_at, "fingerprint":s.fingerprint,
                        "revoked":s.revoked, "sign_count":s.sign_count
                    })
                })
                .collect();
            println!(
                "{}",
                serde_json::json!({"enabled":enabled,"signers":signers})
            );
        }
        Some(verb @ ("enable" | "disable")) => {
            let mut store = crate::cli_open::open_store(db_path);
            store
                .attest_set_enabled(verb == "enable")
                .unwrap_or_else(|e| fail(e));
            println!(
                "attested writes {} in this store; HTTP also requires attest_http",
                if verb == "enable" {
                    "ENABLED"
                } else {
                    "DISABLED"
                }
            );
        }
        Some("revoke") => {
            let id = &args[3]; // Store-free argument admission requires one operand.
            let mut store = crate::cli_open::open_store(db_path);
            if !store.attest_revoke_signer(id).unwrap_or_else(|e| fail(e)) {
                fail("unknown signer");
            }
            println!("signer {id}: REVOKED");
        }
        Some("protect-graph") => {
            let graph = &args[3];
            let mut store = crate::cli_open::open_store(db_path);
            store
                .attest_protect_graph(graph)
                .unwrap_or_else(|e| fail(e));
            println!("{graph}: attested-only (permanent; ordinary writes refused)");
        }
        _ => unreachable!("store-free admission checks signer subcommands"),
    }
}

fn enroll(args: &[String], db_path: &str) {
    let graphs: Vec<String> = values(args, "--graph")
        .into_iter()
        .map(str::to_owned)
        .collect();
    if graphs.is_empty()
        || graphs
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            != graphs.len()
    {
        eprintln!("signer enroll requires explicit distinct --graph values");
        std::process::exit(2);
    }
    let signer = Signer {
        id: required(args, "--credential-id"),
        scheme: required(args, "--scheme"),
        cose_key: quipu::verdict_schemes::b64url_decode(&required(args, "--cose-key"))
            .unwrap_or_else(|e| fail(e)),
        origin: required(args, "--origin"),
        rp_id: required(args, "--rp-id"),
        actor: required(args, "--actor"),
        graphs,
        expires_at: required(args, "--expires-at")
            .parse()
            .unwrap_or_else(|_| fail("expiry must be epoch seconds")),
    };
    let mut store = crate::cli_open::open_store(db_path);
    store
        .attest_enroll_signer(&signer)
        .unwrap_or_else(|e| fail(e));
    let fingerprint = quipu::store::attested_write::key_fingerprint(&signer.cose_key);
    println!(
        "enrolled signer {} fingerprint {fingerprint}; compare with the enrolling device",
        signer.id
    );
    println!("Enrollment did not enable writes or protect any graph.");
}
