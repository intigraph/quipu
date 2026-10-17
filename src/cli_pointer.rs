//! Explicit first graph use. Checkout/sync remains declaration-only.
use crate::cli::{chrono_now, flag_value};

pub fn cmd_use_pointer(args: &[String], db_path: &str) {
    match run(args, db_path) {
        Ok(result) => println!("{}", serde_json::to_string_pretty(&result).unwrap()),
        Err(error) => {
            eprintln!("pointer use refused: {error}");
            std::process::exit(1);
        }
    }
}

fn run(args: &[String], db_path: &str) -> quipu::Result<quipu::share_compose::Composition> {
    let required = |flag| {
        flag_value(args, flag).ok_or_else(|| {
            quipu::Error::InvalidValue(format!("compose --use-pointer requires {flag}"))
        })
    };
    let graph = required("--use-pointer")?;
    let declaration_path = required("--declaration")?;
    let authority_path = required("--authority")?;
    let remote = required("--repo-remote")?;
    let cache_dir = required("--pointer-cache")?;
    let destination = match flag_value(args, "--destination") {
        None | Some("outward") => quipu::share::ShareDestination::Outward,
        Some("internal") => quipu::share::ShareDestination::Internal,
        Some(value) => {
            return Err(quipu::Error::InvalidValue(format!(
                "unknown destination {value}"
            )));
        }
    };
    let read = |path| -> quipu::Result<_> {
        let mut request = quipu::share_transport::read_local(path)?;
        request.destination = destination;
        Ok(request)
    };
    let loader =
        quipu::share_pointer_load::LazyPacks::new(read(declaration_path)?, read(authority_path)?)?;
    let mut cache = quipu::share_pointer_load::DirectoryCache(cache_dir.into());
    let mut store = crate::cli_open::open_store(db_path);
    loader.use_graph(
        &mut store,
        graph,
        &mut cache,
        |_, pointer| {
            let quipu::share::PointerTarget::Pack { sha256, .. } = &pointer.target else {
                unreachable!()
            };
            // Repository identity comes from the receiver's command, never from a
            // downloaded manifest. A missing policy tool fails closed.
            let result = std::process::Command::new("caboodle")
                .args([
                    "packs",
                    "trust",
                    "check",
                    remote,
                    sha256.strip_prefix("sha256:").unwrap_or(sha256),
                    "--pack",
                    &pointer.graph,
                ])
                .output()
                .map_err(|e| {
                    quipu::Error::InvalidValue(format!("caboodle trust unavailable: {e}"))
                })?;
            if !result.status.success() {
                return Err(quipu::Error::InvalidValue(format!(
                    "caboodle trust refused ({}): {}",
                    result.status,
                    String::from_utf8_lossy(&result.stderr).trim()
                )));
            }
            Ok(())
        },
        fetch,
        &chrono_now(),
        flag_value(args, "--actor"),
    )
}

fn fetch(url: &str) -> quipu::Result<Vec<u8>> {
    #[cfg(feature = "remote")]
    {
        quipu::share_pointer_load::fetch_archive(url)
    }
    #[cfg(not(feature = "remote"))]
    {
        let _ = url;
        Err(quipu::Error::InvalidValue(
            "pointer fetch requires the remote feature (verified cache remains usable)".into(),
        ))
    }
}
