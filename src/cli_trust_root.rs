//! `quipu trust-root` — the console ceremony that enrols the FIRST human key
//! (aegis-kzt0ql.9.4). This command is the only caller of
//! `governance::trust_root::bootstrap`; there is no REST or MCP route to it.
//!
//! 1. `quipu trust-root challenge --verifier stiwi --public-key HEX` prints the
//!    exact bytes the new key must sign, and the key's fingerprint.
//! 2. Sign them with the device, then run
//!    `quipu trust-root bootstrap --verifier stiwi --public-key HEX
//!    --pop-signature HEX [--attests POLICY ...]`.
//! 3. COMPARE the printed fingerprint with the one the device shows. A
//!    mismatch means someone bootstrapped first: stop and escalate.
//!
//! Every later key is an amendment signed by an enrolled one, never a second
//! bootstrap. Anyone who can run this binary against the store file can
//! attempt step 2 first; the fingerprint comparison and the bootstrap alert
//! exist for that window (see docs/runbooks/trust-root-ceremony.md).

use quipu::governance::trust_root;

fn flag<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    let i = args.iter().position(|a| a == name)?;
    args.get(i + 1).map(String::as_str)
}

fn flags(args: &[String], name: &str) -> Vec<String> {
    args.windows(2)
        .filter(|w| w[0] == name)
        .map(|w| w[1].clone())
        .collect()
}

fn need(args: &[String], name: &str) -> String {
    flag(args, name).map_or_else(
        || {
            eprintln!("trust-root requires {name}");
            std::process::exit(2);
        },
        str::to_string,
    )
}

fn fail(what: &str, e: impl std::fmt::Display) -> ! {
    eprintln!("{what}: {e}");
    std::process::exit(1);
}

pub fn cmd_trust_root(args: &[String], db_path: &str) {
    match args.get(2).map(String::as_str) {
        Some("challenge") => challenge(args, db_path),
        Some("bootstrap") => bootstrap(args, db_path),
        Some("status") => status(db_path),
        Some("endorse-challenge") => endorse_challenge(args, db_path),
        Some("endorse") => endorse(args, db_path),
        _ => {
            eprintln!(
                "quipu trust-root challenge --verifier NAME --public-key HEX [--db PATH]\n\
                 quipu trust-root bootstrap --verifier NAME --public-key HEX --pop-signature HEX \\\n\
                 \x20   [--attests POLICY]... [--db PATH]\n\
                 quipu trust-root status [--db PATH]\n\
                 quipu trust-root endorse-challenge --registration IRI [--tier agent] [--db PATH]\n\
                 quipu trust-root endorse --registration IRI [--tier agent] --signer NAME \\\n\
                 \x20   --nonce HEX --signature HEX [--db PATH]\n\n\
                 Enrols the FIRST human trust-root key, once per store, ever. Later keys are\n\
                 amendments signed by an enrolled key. Compare the printed fingerprint with\n\
                 the device before trusting the result (docs/runbooks/trust-root-ceremony.md).\n\
                 endorse makes a verifier registration chain to the trust root, so its\n\
                 verdicts can verify as trusted (aegis-c97o)."
            );
            std::process::exit(2);
        }
    }
}

fn challenge(args: &[String], db_path: &str) {
    let store = crate::cli_open::open_store(db_path);
    let verifier = need(args, "--verifier");
    let key = need(args, "--public-key");
    let fingerprint = trust_root::fingerprint(&key).unwrap_or_else(|e| fail("bad key", e));
    let store_id = store.store_id().unwrap_or_else(|e| fail("store id", e));
    if trust_root::ever_bootstrapped(&store).unwrap_or_else(|e| fail("history", e)) {
        fail(
            "refused",
            "a human key has already been enrolled in this store",
        );
    }
    let message = trust_root::bootstrap_message(&store_id, &verifier, &key);
    println!("store        {store_id}");
    println!("fingerprint  {fingerprint}");
    println!("sign exactly these bytes (no trailing newline):");
    println!("{}", String::from_utf8_lossy(&message));
}

fn bootstrap(args: &[String], db_path: &str) {
    let mut store = crate::cli_open::open_store(db_path);
    let now = quipu::time::now_iso();
    match trust_root::bootstrap(
        &mut store,
        &need(args, "--verifier"),
        &need(args, "--public-key"),
        &flags(args, "--attests"),
        &need(args, "--pop-signature"),
        &now,
    ) {
        Ok(e) => {
            println!("ENROLLED     {}", e.registration);
            println!("FINGERPRINT  {}", e.fingerprint);
            println!();
            println!("Now compare that fingerprint with the one your device shows.");
            println!("If they differ, someone else bootstrapped first: STOP and escalate.");
        }
        Err(e) => fail("bootstrap refused", e),
    }
}

fn status(db_path: &str) {
    let store = crate::cli_open::open_store(db_path);
    let ever = trust_root::ever_bootstrapped(&store).unwrap_or_else(|e| fail("history", e));
    let keys = trust_root::human_keys(&store).unwrap_or_else(|e| fail("registry", e));
    println!("bootstrapped {}", if ever { "yes" } else { "no" });
    for (registration, verifier, key) in keys {
        let fp = trust_root::fingerprint(&key).unwrap_or_else(|_| "(not an ed25519 key)".into());
        println!("{verifier}\t{fp}\t{registration}");
    }
    let pending = trust_root::unendorsed(&store).unwrap_or_else(|e| fail("registry", e));
    if !pending.is_empty() {
        println!(
            "unendorsed   {} (verify nothing until endorsed)",
            pending.len()
        );
        for (registration, verifier) in pending {
            println!("  {verifier}\t{registration}");
        }
    }
}

fn tier(args: &[String]) -> String {
    flag(args, "--tier").unwrap_or("agent").to_string()
}

fn endorse_challenge(args: &[String], db_path: &str) {
    let store = crate::cli_open::open_store(db_path);
    let registration = need(args, "--registration");
    let c = trust_root::endorse_challenge(&store, &registration, &tier(args))
        .unwrap_or_else(|e| fail("refused", e));
    // What is being endorsed: check every line before signing.
    println!("store        {}", c.store_id);
    println!("registration {registration}");
    println!("tier         {}", c.tier);
    if c.keys.is_empty() {
        println!("key          (none: the registration holds no aegis:publicKey)");
    }
    for key in &c.keys {
        match trust_root::fingerprint(key) {
            Ok(fp) => println!("key          {fp}"),
            Err(_) => println!("key          {key} (not an ed25519 key)"),
        }
    }
    println!("digest       {}", c.digest);
    println!("nonce        {}", c.nonce);
    println!(
        "sign exactly these bytes with an enrolled human trust-root key (no trailing newline):"
    );
    println!("{}", String::from_utf8_lossy(&c.message));
}

fn endorse(args: &[String], db_path: &str) {
    let mut store = crate::cli_open::open_store(db_path);
    let registration = need(args, "--registration");
    match trust_root::endorse(
        &mut store,
        &registration,
        &tier(args),
        &need(args, "--signer"),
        &need(args, "--nonce"),
        &need(args, "--signature"),
        &quipu::time::now_iso(),
    ) {
        Ok(amendment) => {
            println!("ENDORSED     {registration}");
            println!("AMENDMENT    {amendment}");
        }
        Err(e) => fail("endorse refused", e),
    }
}
