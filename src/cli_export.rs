//! RDF export command parsing.

use std::io::{self, Write};

use oxrdfio::RdfFormat;

pub fn cmd_export(args: &[String], db_path: &str) {
    let format = args
        .windows(2)
        .find(|w| w[0] == "--format")
        .map_or("ntriples", |w| w[1].as_str());
    let rdf_format = match format {
        "ntriples" | "nt" => RdfFormat::NTriples,
        "turtle" | "ttl" => RdfFormat::Turtle,
        _ => {
            eprintln!("unknown format: {format} (try: ntriples, turtle)");
            std::process::exit(1);
        }
    };
    let value = |flag| {
        args.windows(2)
            .find(|w| w[0] == flag)
            .map(|w| w[1].as_str())
    };
    let graph = value("--graph");
    let group = value("--group-id");
    let construct = value("--construct");
    if [graph.is_some(), group.is_some(), construct.is_some()]
        .into_iter()
        .filter(|v| *v)
        .count()
        > 1
    {
        eprintln!("export accepts only one of --graph, --group-id, or --construct");
        std::process::exit(1);
    }
    let store = if args.iter().any(|arg| arg == "--read-only") {
        open_snapshot(db_path, crate::cli_open::config()).unwrap_or_else(|error| {
            eprintln!("error opening read-only export snapshot: {error}");
            std::process::exit(1);
        })
    } else {
        crate::cli_open::open_store(db_path)
    };
    let exported = match (graph, group, construct) {
        (Some(iri), None, None) => {
            quipu::export_rdf_subset(&store, rdf_format, Some(iri)).map(|v| v.0)
        }
        (None, Some(id), None) => quipu::export_rdf_group(&store, rdf_format, id).map(|v| v.0),
        (None, None, Some(query)) => {
            quipu::export_rdf_construct(&store, rdf_format, query).map(|v| v.0)
        }
        (None, None, None) => quipu::export_rdf(&store, rdf_format),
        _ => unreachable!("mutually exclusive export scopes checked above"),
    };
    match exported {
        Ok(bytes) => io::stdout().write_all(&bytes).unwrap(),
        Err(error) => {
            eprintln!("error exporting: {error}");
            std::process::exit(1);
        }
    }
}

/// A cold, custodian-held snapshot only: never run schema setup, index creation,
/// or a WAL-mode fallback. The caller must keep the source cold
/// for the entire export; this check does not fence an external writer.
fn open_snapshot(db_path: &str, config: &quipu::QuipuConfig) -> Result<quipu::Store, String> {
    if !config.attachments.is_empty() {
        return Err(
            "--read-only exports one cold local snapshot; configured attachments are unsupported"
                .into(),
        );
    }
    let path = std::fs::canonicalize(db_path).map_err(|error| error.to_string())?;
    if !path.is_file() {
        return Err("snapshot must be an existing regular file".into());
    }
    let path = path.to_str().ok_or("snapshot path must be UTF-8")?;
    for suffix in ["-wal", "-shm", "-journal"] {
        match std::fs::symlink_metadata(format!("{path}{suffix}")) {
            Ok(_) => return Err(format!("cold snapshot required: {suffix} sidecar exists")),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.to_string()),
        }
    }
    // Use the immutable URI directly rather than open_immutable's deliberate
    // WAL fallback: this export must never create WAL/shared-memory siblings.
    let escaped = path
        .replace('%', "%25")
        .replace('?', "%3f")
        .replace('#', "%23");
    let uri = format!("file:{escaped}?mode=ro&immutable=1");
    let mut store = quipu::Store::open_read_only(&uri).map_err(|error| error.to_string())?;
    store.search_config_mut().clone_from(&config.search);
    Ok(store)
}

#[cfg(test)]
mod tests {
    use super::open_snapshot;

    #[test]
    fn readonly_export_handle_refuses_writes_without_changing_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("snapshot?#%.db");
        drop(quipu::Store::open(path.to_str().unwrap()).unwrap());
        let before = std::fs::read(&path).unwrap();
        let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
        let store = open_snapshot(path.to_str().unwrap(), &quipu::QuipuConfig::default()).unwrap();
        assert!(store.graph_create("urn:must-not-write").is_err());
        drop(store);
        assert_eq!(std::fs::read(&path).unwrap(), before);
        assert_eq!(
            std::fs::metadata(&path).unwrap().modified().unwrap(),
            modified
        );
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
        // The default writable API retains its existing behavior.
        let store = quipu::Store::open(path.to_str().unwrap()).unwrap();
        store.graph_create("urn:can-write").unwrap();
    }

    #[test]
    fn readonly_export_refuses_sidecars_and_missing_inputs_without_creation() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("snapshot.db");
        let config = quipu::QuipuConfig::default();
        assert!(open_snapshot(path.to_str().unwrap(), &config).is_err());
        assert!(!path.exists());
        drop(quipu::Store::open(path.to_str().unwrap()).unwrap());
        let before = std::fs::read(&path).unwrap();
        for suffix in ["-wal", "-shm", "-journal"] {
            let sidecar = format!("{}{suffix}", path.display());
            std::fs::write(&sidecar, b"retained").unwrap();
            assert!(open_snapshot(path.to_str().unwrap(), &config).is_err());
            assert_eq!(std::fs::read(&sidecar).unwrap(), b"retained");
            std::fs::remove_file(sidecar).unwrap();
        }
        assert_eq!(std::fs::read(&path).unwrap(), before);
        let mut attached = config;
        attached
            .attachments
            .push(quipu::config::AttachmentConfig::new("archive", &path));
        assert!(open_snapshot(path.to_str().unwrap(), &attached).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }
}
