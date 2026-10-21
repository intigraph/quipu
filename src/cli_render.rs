//! Client-side filesystem publication; the server only prepares bytes.

use quipu::render::{RenderPlan, RenderRequest, RenderSpec};
use std::path::Path;

const HELP: &str = "qp render <spec-IRI> --dest <owned-dir> [--expected-tx N] [--valid-at <timestamp>] [--dry-run] [--db <path>|--server <url>]\nqp render --query <SELECT-or-CONSTRUCT> [--template <text>] [--layout single|per-entity] [--path <template>] --dest <owned-dir> [--expected-tx N] [--dry-run]\nThe destination parent must exist. Unknown or edited files refuse. Templates cannot read files or use the network. --expected-tx requires the prepared plan's source-local transaction before publishing; it does not select an older transaction.";

pub fn run(args: &[String], db_path: &str) {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("{HELP}");
        return;
    }
    if let Err(error) = execute(args, db_path) {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

fn execute(args: &[String], db_path: &str) -> quipu::Result<()> {
    let invalid = |text: &str| quipu::Error::InvalidValue(text.into());
    let mut values = std::collections::BTreeMap::new();
    let mut spec = None;
    let mut dry_run = false;
    let mut i = 2;
    while i < args.len() {
        let key = &args[i];
        if key == "--dry-run" {
            if dry_run {
                return Err(invalid("duplicate --dry-run"));
            }
            dry_run = true;
            i += 1;
            continue;
        }
        if matches!(
            key.as_str(),
            "--dest"
                | "--graph"
                | "--valid-at"
                | "--db"
                | "--server"
                | "--query"
                | "--template"
                | "--layout"
                | "--path"
                | "--expected-tx"
        ) {
            let value = args
                .get(i + 1)
                .filter(|v| !v.starts_with("--"))
                .ok_or_else(|| invalid("flag requires a value"))?;
            if values.insert(key.as_str(), value.as_str()).is_some() {
                return Err(invalid("duplicate flag"));
            }
            i += 2;
        } else if key.starts_with('-') || spec.replace(key.clone()).is_some() {
            return Err(invalid("unknown argument or multiple specs"));
        } else {
            i += 1;
        }
    }
    let dest = values
        .get("--dest")
        .ok_or_else(|| invalid("--dest is required"))?;
    if values.contains_key("--db") && values.contains_key("--server") {
        return Err(invalid("choose --db or --server"));
    }
    let expected_tx = values
        .get("--expected-tx")
        .map(|value| {
            value
                .parse::<i64>()
                .ok()
                .filter(|tx| *tx > 0)
                .ok_or_else(|| invalid("--expected-tx requires a positive signed 64-bit integer"))
        })
        .transpose()?;
    let adhoc = values.get("--query").map(|query| RenderSpec {
        query: (*query).into(),
        format: "markdown".into(),
        layout: values.get("--layout").unwrap_or(&"single").to_string(),
        path: values.get("--path").unwrap_or(&"report.md").to_string(),
        template: values.get("--template").map(ToString::to_string),
        includes: Vec::new(),
        provenance: false,
    });
    if spec.is_some()
        && values
            .keys()
            .any(|k| matches!(*k, "--query" | "--template" | "--layout" | "--path"))
    {
        return Err(invalid("stored spec cannot be overridden"));
    }
    let request = RenderRequest {
        spec,
        adhoc,
        valid_at: values.get("--valid-at").map(ToString::to_string),
        graph: values.get("--graph").map(ToString::to_string),
    };
    let plan = if let Some(url) = values.get("--server") {
        remote(url, &request)?
    } else {
        quipu::render::prepare(&crate::cli_open::open_store(db_path), &request)?
    };
    if expected_tx.is_some_and(|tx| tx != plan.source_tx) {
        return Err(invalid(
            "prepared source transaction differs from --expected-tx",
        ));
    }
    // Prints the complete file list and path-level content/mode diff. Dry-run
    // deliberately performs no mkdir, lockfile or manifest/event write.
    let changes = quipu::render::publish::publish(&plan, Path::new(dest), dry_run)?;
    println!(
        "{}",
        serde_json::json!({"dry_run":dry_run,"spec":plan.spec,"source_tx":plan.source_tx,
        "files":plan.files.keys().collect::<Vec<_>>(),"diff":changes})
    );
    Ok(())
}

#[cfg(feature = "remote")]
fn remote(url: &str, request: &RenderRequest) -> quipu::Result<RenderPlan> {
    use std::io::Read;
    let error =
        |e: &dyn std::fmt::Display| quipu::Error::InvalidValue(format!("remote render: {e}"));
    let endpoint = format!("{}/render", url.trim_end_matches('/'));
    if !endpoint.starts_with("https://") && !endpoint.starts_with("http://") {
        return Err(error(&"server URL requires http or https"));
    }
    let agent = ureq::AgentBuilder::new()
        .redirects(0)
        .timeout(std::time::Duration::from_secs(30))
        .build();
    let mut post = agent
        .post(&endpoint)
        .set("Content-Type", "application/json")
        .set("X-Quipu-Client", "cli-render");
    let token = render_credential()?;
    post = post.set("Authorization", &format!("Bearer {token}"));
    let response = post
        .send_string(&serde_json::to_string(request).map_err(|e| error(&e))?)
        .map_err(|e| error(&e))?;
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take((quipu::render::MAX_BYTES * 2 + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|e| error(&e))?;
    if bytes.len() > quipu::render::MAX_BYTES * 2 {
        return Err(error(&"response cap exceeded"));
    }
    let plan: RenderPlan = serde_json::from_slice(&bytes).map_err(|e| error(&e))?;
    if plan.spec != request.spec.as_deref().unwrap_or("adhoc") || plan.valid_at != request.valid_at
    {
        return Err(error(&"response spec/time stamp differs from the request"));
    }
    plan.validate()?;
    Ok(plan)
}

#[cfg(feature = "remote")]
fn render_credential() -> quipu::Result<String> {
    use std::io::Read;
    let fail = || {
        quipu::Error::InvalidValue("remote render requires a provisioned client credential".into())
    };
    if let Ok(value) = std::env::var("QUIPU_AUTH_TOKEN")
        && !value.trim().is_empty()
    {
        return checked_credential(&value);
    }
    let path = std::env::var_os("QUIPU_AUTH_TOKEN_FILE")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .map(|home| std::path::PathBuf::from(home).join(".config/quipu/token"))
        })
        .ok_or_else(fail)?;
    let mut value = String::new();
    std::fs::File::open(path)
        .map_err(|_| fail())?
        .take(16_385)
        .read_to_string(&mut value)
        .map_err(|_| fail())?;
    checked_credential(&value)
}

#[cfg(feature = "remote")]
fn checked_credential(value: &str) -> quipu::Result<String> {
    let token = value.trim();
    if token.is_empty() || value.len() > 16_384 || !token.bytes().all(|b| b.is_ascii_graphic()) {
        // ureq includes a rejected header's complete value in BadHeader.
        // Validate before handing a credential to that error-producing layer.
        return Err(quipu::Error::InvalidValue(
            "remote render requires a valid provisioned client credential".into(),
        ));
    }
    Ok(token.to_owned())
}
#[cfg(not(feature = "remote"))]
fn remote(_: &str, _: &RenderRequest) -> quipu::Result<RenderPlan> {
    Err(quipu::Error::InvalidValue(
        "--server requires a build with the remote feature".into(),
    ))
}
