//! Store-free CLI for the same authenticated HTTP fluent adapter.
fn fail(message: &str) -> ! {
    eprintln!("fluent: {message}");
    std::process::exit(1)
}

#[cfg(not(feature = "remote"))]
pub(crate) fn run(_: &[String]) {
    fail("requires the remote feature; no local database write path exists")
}

#[cfg(feature = "remote")]
pub(crate) fn run(args: &[String]) {
    if let Err(message) = request(args) {
        fail(&message);
    }
}

#[cfg(feature = "remote")]
fn request(args: &[String]) -> Result<(), String> {
    use std::io::Read;
    let flag = |name| crate::cli::flag_value(args, name);
    if args.iter().any(|arg| arg == "--db") {
        return Err("--db is invalid: fluent commands use the HTTP route only".into());
    }
    for name in ["--server", "--graph", "--fluent"] {
        if args.iter().filter(|arg| arg.as_str() == name).count() > 1 {
            return Err(format!("duplicate option {name}"));
        }
    }
    // Syntax admission allows options before or after operands.
    let mut positionals = Vec::new();
    let mut index = 1;
    while index < args.len() {
        if args[index].starts_with("--") {
            index += 2;
        } else {
            positionals.push(args[index].as_str());
            index += 1;
        }
    }
    let server = flag("--server").ok_or("--server is required")?;
    let mut url = url::Url::parse(server).map_err(|_| "invalid --server URL")?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path() != "/"
    {
        return Err("--server must be an http(s) origin without credentials/query/path".into());
    }
    let agent = ureq::AgentBuilder::new()
        .redirects(0)
        .timeout(std::time::Duration::from_secs(30))
        .build();
    let response = match positionals.get(1).copied() {
        Some("append") => {
            let path = positionals.get(2).ok_or("append requires a JSON file")?;
            let mut bytes = Vec::new();
            std::fs::File::open(path)
                .map_err(|_| "cannot open append JSON")?
                .take(65537)
                .read_to_end(&mut bytes)
                .map_err(|_| "cannot read append JSON")?;
            if bytes.len() > 65536 {
                return Err("append JSON exceeds 64KiB".into());
            }
            let _: quipu::fluent::transport::AppendRequest =
                serde_json::from_slice(&bytes).map_err(|_| "invalid append JSON contract")?;
            let token = token()?;
            url.set_path("/fluent/observations");
            agent
                .post(url.as_str())
                .set("Authorization", &format!("Bearer {token}"))
                .set("Content-Type", "application/json")
                .send_bytes(&bytes)
        }
        Some("current") => {
            url.set_path("/fluent/current");
            url.query_pairs_mut()
                .append_pair("graph", flag("--graph").ok_or("--graph is required")?)
                .append_pair("fluent", flag("--fluent").ok_or("--fluent is required")?);
            let mut request = agent.get(url.as_str());
            // Open-read deployments need no token; a configured token is still
            // sent for installations with scoped read authentication.
            if std::env::var_os("QUIPU_AUTH_TOKEN").is_some()
                || std::env::var_os("QUIPU_AUTH_TOKEN_FILE").is_some()
            {
                request = request.set("Authorization", &format!("Bearer {}", token()?));
            }
            request.call()
        }
        _ => return Err("expected append or current".into()),
    }
    .map_err(|error| match error {
        ureq::Error::Status(status, _) => format!("server refused fluent request: HTTP {status}"),
        ureq::Error::Transport(_) => {
            "HTTP transport failed; outcome may be indeterminate; do not change event on retry"
                .into()
        }
    })?;
    let mut body = String::new();
    response
        .into_reader()
        .take(1024 * 1024 + 1)
        .read_to_string(&mut body)
        .map_err(|_| "cannot read fluent response; outcome may be indeterminate")?;
    if body.len() > 1024 * 1024 {
        return Err("fluent response exceeds 1MiB".into());
    }
    println!("{body}");
    Ok(())
}

#[cfg(feature = "remote")]
fn token() -> Result<String, String> {
    use std::io::Read;
    if let Ok(value) = std::env::var("QUIPU_AUTH_TOKEN")
        && !value.trim().is_empty()
    {
        return Ok(value.trim().into());
    }
    let path =
        std::env::var_os("QUIPU_AUTH_TOKEN_FILE").ok_or("named bearer credential required")?;
    let mut value = String::new();
    std::fs::File::open(path)
        .map_err(|_| "cannot open token file")?
        .take(16385)
        .read_to_string(&mut value)
        .map_err(|_| "cannot read token file")?;
    if value.len() > 16384 || value.trim().is_empty() {
        return Err("token file is empty or too large".into());
    }
    Ok(value.trim().into())
}
