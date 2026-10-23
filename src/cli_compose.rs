//! Compose local verified packs into a named inspection dataset.

use crate::cli::{chrono_now, flag_value};

pub fn cmd_compose(args: &[String], db_path: &str) {
    if flag_value(args, "--use-pointer").is_some() {
        return crate::cli_pointer::cmd_use_pointer(args, db_path);
    }
    if let Some(repo) = flag_value(args, "--repo") {
        return cmd_compose_repo(args, db_path, repo);
    }
    let result = run(args, db_path);
    match result {
        Ok(result) => {
            println!("{}", serde_json::to_string_pretty(&result).unwrap());
            if result.outcome == "quarantined" {
                std::process::exit(2);
            }
        }
        Err(error) => {
            eprintln!("composition error: {error}");
            std::process::exit(1);
        }
    }
}

fn run(args: &[String], db_path: &str) -> quipu::Result<quipu::share_compose::Composition> {
    let mut references = Vec::new();
    let mut index = 2;
    while index < args.len() {
        match args[index].as_str() {
            "--db" | "--shapes-from" | "--actor" | "--destination" | "--repo" => {
                if args.get(index + 1).is_none() {
                    return Err(quipu::Error::InvalidValue(format!(
                        "{} needs a value",
                        args[index]
                    )));
                }
                index += 2;
            }
            "--rollback" => index += 1,
            value if value.starts_with('-') => {
                return Err(quipu::Error::InvalidValue(format!(
                    "unknown compose option: {value}"
                )));
            }
            value => {
                references.push(value);
                index += 1;
            }
        }
    }
    let authority = flag_value(args, "--shapes-from")
        .map(|reference| {
            references
                .iter()
                .position(|r| *r == reference)
                .ok_or_else(|| {
                    quipu::Error::InvalidValue(
                        "--shapes-from must name one of the input paths".into(),
                    )
                })
        })
        .transpose()?;
    let destination = match flag_value(args, "--destination") {
        None | Some("outward") => quipu::share::ShareDestination::Outward,
        Some("internal") => quipu::share::ShareDestination::Internal,
        Some(value) => {
            return Err(quipu::Error::InvalidValue(format!(
                "unknown destination: {value}"
            )));
        }
    };
    let requests = references
        .into_iter()
        .map(|reference| {
            crate::cli_pack::warn_deprecated_extension(reference);
            let mut request = quipu::share_transport::read_local(reference)?;
            request.destination = destination.clone();
            Ok(request)
        })
        .collect::<quipu::Result<Vec<_>>>()?;
    let mut store = crate::cli_open::open_store(db_path);
    quipu::share_compose::compose(
        &mut store,
        &requests,
        authority,
        &chrono_now(),
        flag_value(args, "--actor"),
    )
}

/// Pendant autoload repository mode: `--repo R <pack-key>=<path>...` composes and
/// moves `urn:quipu:repo:R:current`; `--repo R --rollback` moves it back.
fn cmd_compose_repo(args: &[String], db_path: &str, repo: &str) {
    let outcome = if args.iter().any(|a| a == "--revoke-tombstone") {
        run_revoke(args, db_path, repo)
    } else if args.iter().any(|a| a == "--rollback") {
        run_rollback(args, db_path, repo)
    } else {
        run_repo(args, db_path, repo)
    };
    match outcome {
        Ok((json, quarantined)) => {
            println!("{json}");
            if quarantined {
                std::process::exit(2);
            }
        }
        Err(error) => {
            eprintln!("composition error: {error}");
            std::process::exit(1);
        }
    }
}

fn run_revoke(args: &[String], db_path: &str, repo: &str) -> quipu::Result<(String, bool)> {
    let mut seen = std::collections::BTreeSet::new();
    let mut index = 2;
    while index < args.len() {
        let option = args[index].as_str();
        if !matches!(
            option,
            "--db" | "--repo" | "--actor" | "--revoke-tombstone" | "--pack-key" | "--reason"
        ) || !seen.insert(option)
            || args.get(index + 1).is_none_or(|v| v.starts_with("--"))
        {
            return Err(quipu::Error::InvalidValue(format!(
                "invalid or repeated revocation option, or missing value: {option}"
            )));
        }
        index += 2;
    }
    let required = |name| {
        flag_value(args, name)
            .ok_or_else(|| quipu::Error::InvalidValue(format!("revocation requires {name}")))
    };
    let (key, statement, reason) = (
        required("--pack-key")?,
        required("--revoke-tombstone")?,
        required("--reason")?,
    );
    let mut store = crate::cli_open::open_store(db_path);
    let result = quipu::share_current::revoke_tombstone(
        &mut store,
        repo,
        key,
        statement,
        reason,
        &chrono_now(),
        flag_value(args, "--actor"),
    )?;
    Ok((serde_json::to_string_pretty(&result).unwrap(), false))
}

fn run_rollback(args: &[String], db_path: &str, repo: &str) -> quipu::Result<(String, bool)> {
    if positional(args)?.next().is_some() {
        return Err(quipu::Error::InvalidValue(
            "--rollback takes no packs; it moves CURRENT back to the previous snapshot".into(),
        ));
    }
    let mut store = crate::cli_open::open_store(db_path);
    let result = quipu::share_current::rollback_current(
        &mut store,
        repo,
        &chrono_now(),
        flag_value(args, "--actor"),
    )?;
    Ok((serde_json::to_string_pretty(&result).unwrap(), false))
}

fn run_repo(args: &[String], db_path: &str, repo: &str) -> quipu::Result<(String, bool)> {
    let mut keys = Vec::new();
    let mut paths = Vec::new();
    for value in positional(args)? {
        let (key, path) = value.split_once('=').ok_or_else(|| {
            quipu::Error::InvalidValue(format!(
                "with --repo every pack is <pack-key>=<path>, the key naming the pack across \
                 commits (got {value:?})"
            ))
        })?;
        keys.push(key.to_string());
        paths.push(path);
    }
    if paths.is_empty() {
        return Err(quipu::Error::InvalidValue(
            "--repo needs at least one <pack-key>=<path>".into(),
        ));
    }
    let authority = flag_value(args, "--shapes-from")
        .map(|key| {
            keys.iter().position(|k| k == key).ok_or_else(|| {
                quipu::Error::InvalidValue("--shapes-from must name one of the pack keys".into())
            })
        })
        .transpose()?;
    let destination = match flag_value(args, "--destination") {
        None | Some("outward") => quipu::share::ShareDestination::Outward,
        Some("internal") => quipu::share::ShareDestination::Internal,
        Some(value) => {
            return Err(quipu::Error::InvalidValue(format!(
                "unknown destination: {value}"
            )));
        }
    };
    let requests = paths
        .into_iter()
        .map(|path| {
            crate::cli_pack::warn_deprecated_extension(path);
            let mut request = quipu::share_transport::read_local(path)?;
            request.destination = destination.clone();
            Ok(request)
        })
        .collect::<quipu::Result<Vec<_>>>()?;
    let mut store = crate::cli_open::open_store(db_path);
    let result = quipu::share_current::compose_current(
        &mut store,
        repo,
        &keys,
        &requests,
        authority,
        &chrono_now(),
        flag_value(args, "--actor"),
    )?;
    let quarantined = result.composition.outcome == "quarantined";
    Ok((serde_json::to_string_pretty(&result).unwrap(), quarantined))
}

/// Positional (non-flag) arguments after the subcommand.
fn positional(args: &[String]) -> quipu::Result<impl Iterator<Item = &str>> {
    let mut out = Vec::new();
    let mut index = 2;
    while index < args.len() {
        match args[index].as_str() {
            "--db" | "--shapes-from" | "--actor" | "--destination" | "--repo" => index += 2,
            "--rollback" => index += 1,
            value if value.starts_with('-') => {
                return Err(quipu::Error::InvalidValue(format!(
                    "unknown compose option: {value}"
                )));
            }
            value => {
                out.push(value);
                index += 1;
            }
        }
    }
    Ok(out.into_iter())
}
