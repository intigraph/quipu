//! MCP enters the server in this executable; no companion lookup or exec.
pub(crate) fn run(args: &[String]) -> ! {
    if !cfg!(all(feature = "server", feature = "onnx")) {
        crate::cli_server::run(&[]);
        std::process::exit(1);
    }
    if !args
        .iter()
        .any(|a| matches!(a.as_str(), "--help" | "-h" | "--version" | "-V"))
        && let Err(error) = ensure_db_parent(args)
    {
        eprintln!("{error}");
        std::process::exit(1);
    }
    let mut server_args = vec!["quipu".to_owned(), "--mcp-stdio".to_owned()];
    server_args.extend_from_slice(args);
    crate::cli_server::run(&server_args);
    std::process::exit(0);
}

/// Create the directory a `--db` path lives in (aegis-gys8sx).
///
/// The documented local setup is `quipu mcp --db ${workspaceFolder}/.quipu/local.db`,
/// and in a fresh workspace `.quipu/` does not exist yet, so the server exited
/// before `initialize` with "unable to open database file". An editor shows that
/// as a server that will not start, on first use, with nothing in the workspace
/// to explain it. Only the parent is created: the store itself is still created
/// by the server, and `quipu-server --db` keeps its strict behaviour for every
/// other caller.
fn ensure_db_parent(args: &[String]) -> Result<(), String> {
    let Some(db) = db_arg(args) else {
        return Ok(());
    };
    match std::path::Path::new(db).parent() {
        Some(dir) if !dir.as_os_str().is_empty() && !dir.exists() => std::fs::create_dir_all(dir)
            .map_err(|error| format!("cannot create store directory {}: {error}", dir.display())),
        _ => Ok(()),
    }
}

/// The value of `--db <path>` or `--db=<path>`, the last one winning.
fn db_arg(args: &[String]) -> Option<&str> {
    let mut found = None;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == "--db" {
            found = iter.next().map(String::as_str);
        } else if let Some(value) = arg.strip_prefix("--db=") {
            found = Some(value);
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn a_fresh_workspace_gets_its_store_directory() {
        let root = tempfile::tempdir().unwrap();
        let db = root.path().join(".quipu").join("local.db");
        ensure_db_parent(&args(&["--db", db.to_str().unwrap()])).unwrap();
        assert!(db.parent().unwrap().is_dir());
        assert!(
            !db.exists(),
            "only the directory is created; the server creates the store"
        );
    }

    #[test]
    fn the_equals_form_and_an_existing_directory_both_work() {
        let root = tempfile::tempdir().unwrap();
        let db = root.path().join("a").join("b").join("x.db");
        let flag = format!("--db={}", db.display());
        ensure_db_parent(&args(&[&flag])).unwrap();
        ensure_db_parent(&args(&[&flag])).unwrap();
        assert!(db.parent().unwrap().is_dir());
    }

    #[test]
    fn no_db_argument_or_a_bare_file_name_touches_nothing() {
        assert_eq!(db_arg(&args(&["--mcp-token-file", "t"])), None);
        ensure_db_parent(&args(&[])).unwrap();
        ensure_db_parent(&args(&["--db", "local.db"])).unwrap();
    }

    #[test]
    fn an_uncreatable_directory_is_reported_not_ignored() {
        let root = tempfile::tempdir().unwrap();
        let blocker = root.path().join("file");
        std::fs::write(&blocker, b"").unwrap();
        let db = blocker.join("sub").join("x.db");
        let error = ensure_db_parent(&args(&["--db", db.to_str().unwrap()])).unwrap_err();
        assert!(error.contains("cannot create store directory"), "{error}");
    }
}
