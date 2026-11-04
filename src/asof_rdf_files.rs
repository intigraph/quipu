//! Private, finite offline file boundary for the as-of RDF adapter.
//!
//! Input is JSONL of `{event,data}` pairs; `data` is the EXACT JSON string
//! received from the transport, so re-encoding cannot break the digest chain.

use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::json;

use super::{Binding, Limits, Receipt, convert, invalid};
use crate::applier::Source;
use crate::{Result, Store};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    store_id: String,
    log_epoch: String,
    watermark_tx: i64,
    graph: String,
    shapes_hash: String,
    max_input_bytes: usize,
    max_frames: usize,
    max_disk_bytes: u64,
    wall_seconds: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Frame {
    event: String,
    data: String,
}

fn private(path: &Path, directory: bool) -> Result<fs::Metadata> {
    let metadata = fs::symlink_metadata(path).map_err(|_| invalid("private input unavailable"))?;
    if metadata.file_type().is_symlink()
        || metadata.is_dir() != directory
        || (!directory && !metadata.is_file())
        || metadata.permissions().mode() & 0o077 != 0
        || metadata.uid() != rustix::process::geteuid().as_raw()
    {
        return Err(invalid(
            "as-of RDF paths must be private, owned and non-symlink",
        ));
    }
    Ok(metadata)
}

fn read_private(path: &Path) -> Result<File> {
    let before = private(path, false)?;
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .map_err(|_| invalid("private input open refused"))?;
    let after = file
        .metadata()
        .map_err(|_| invalid("private input stat failed"))?;
    if before.dev() != after.dev() || before.ino() != after.ino() {
        return Err(invalid("private input changed during open"));
    }
    Ok(file)
}

fn create(path: &Path) -> Result<File> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .map_err(|_| invalid("as-of RDF refuses replacing an output"))
}

struct Frames {
    reader: BufReader<File>,
    deadline: Instant,
}

impl Iterator for Frames {
    type Item = Result<(String, String)>;

    fn next(&mut self) -> Option<Self::Item> {
        if Instant::now() >= self.deadline {
            return Some(Err(invalid("as-of RDF wall budget exceeded")));
        }
        // Worst case JSON escaping expands data sixfold. Bound allocation
        // BEFORE reading a line, rather than checking an unbounded read_line.
        let ceiling = crate::feed_parts::MAX_FRAME_BYTES * 6 + 1024;
        let mut bytes = Vec::new();
        match self
            .reader
            .by_ref()
            .take((ceiling + 1) as u64)
            .read_until(b'\n', &mut bytes)
        {
            Ok(0) => None,
            Ok(_) if bytes.len() > ceiling => {
                Some(Err(invalid("as-of RDF encoded frame exceeds limit")))
            }
            Ok(_) => Some(
                serde_json::from_slice::<Frame>(&bytes)
                    .map(|frame| (frame.event, frame.data))
                    .map_err(|_| invalid("as-of RDF input is not an event/data JSONL frame")),
            ),
            Err(_) => Some(Err(invalid("as-of RDF frame read failed"))),
        }
    }
}

fn usage(scratch: &Path, input_bytes: u64, output_bytes: u64, temp_reserve: u64) -> Result<u64> {
    let mut bytes = input_bytes
        .checked_add(output_bytes)
        .and_then(|v| v.checked_add(temp_reserve))
        .ok_or_else(|| invalid("as-of RDF disk count overflow"))?;
    for name in [
        "replica.db",
        "replica.db-wal",
        "replica.db-shm",
        "replica.db-journal",
    ] {
        match fs::symlink_metadata(scratch.join(name)) {
            Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {
                bytes = bytes
                    .checked_add(metadata.len())
                    .ok_or_else(|| invalid("as-of RDF disk count overflow"))?;
            }
            Ok(_) => return Err(invalid("as-of RDF scratch artifact changed type")),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(invalid("as-of RDF scratch usage is unknown")),
        }
    }
    Ok(bytes)
}

/// Convert retained, authenticated acquisition frames in a new private scratch
/// directory. No network, daemon, existing-store mutation or cleanup is done.
/// `SQLITE_TMPDIR` must already name that same canonical private directory.
/// The input and request must be private regular files; the scratch must be
/// empty. On error retain the partial artifacts, without a completion receipt.
///
/// # Errors
/// Refuses unsafe paths, malformed bindings, any exhausted resource bound or
/// anything refused by the shared applier/RDF converter.
pub fn run(request: &Path, input: &Path, scratch: &Path) -> Result<Receipt> {
    let mut bytes = Vec::new();
    read_private(request)?
        .take(65537)
        .read_to_end(&mut bytes)
        .map_err(|_| invalid("as-of RDF request read failed"))?;
    if bytes.len() > 65536 {
        return Err(invalid("as-of RDF request exceeds limit"));
    }
    let request: Request =
        serde_json::from_slice(&bytes).map_err(|_| invalid("as-of RDF request is malformed"))?;
    let input = read_private(input)?;
    let input_bytes = input
        .metadata()
        .map_err(|_| invalid("as-of RDF input stat failed"))?
        .len();
    private(scratch, true)?;
    if !scratch.is_absolute()
        || fs::canonicalize(scratch).ok().as_deref() != Some(scratch)
        || std::env::var_os("SQLITE_TMPDIR")
            .map(PathBuf::from)
            .as_deref()
            != Some(scratch)
        || fs::read_dir(scratch)
            .map_err(|_| invalid("as-of RDF scratch unreadable"))?
            .next()
            .is_some()
    {
        return Err(invalid(
            "as-of RDF requires an empty canonical scratch and matching SQLITE_TMPDIR",
        ));
    }
    if request.max_disk_bytes < 64 * 1024 * 1024
        || request.wall_seconds == 0
        || request.wall_seconds > 1200
        || input_bytes > request.max_disk_bytes / 2
        || oxrdf::NamedNode::new(request.graph.clone()).is_err()
    {
        return Err(invalid(
            "as-of RDF request scope or resource bounds are invalid",
        ));
    }
    let deadline = Instant::now()
        .checked_add(Duration::from_secs(request.wall_seconds))
        .ok_or_else(|| invalid("as-of RDF deadline overflow"))?;
    let database = scratch.join("replica.db");
    drop(create(&database)?);
    let mut store = Store::open(
        database
            .to_str()
            .ok_or_else(|| invalid("non-UTF8 scratch"))?,
    )?;
    // Reserve a quarter of the total budget for temporary SQLite work,
    // including the shared applier's canonical record sort. It stays on the
    // explicitly selected filesystem even when SQLite unlinks its temp file.
    let temp_reserve = request.max_disk_bytes / 4;
    let page_size: i64 = store
        .conn
        .pragma_query_value(None, "page_size", |r| r.get(0))?;
    let page_size = u64::try_from(page_size)
        .ok()
        .filter(|n| *n > 0)
        .ok_or_else(|| invalid("invalid SQLite page size"))?;
    let pages = (temp_reserve / page_size).min(u64::from(u32::MAX));
    store.conn.execute_batch(&format!(
        "PRAGMA temp_store=FILE; PRAGMA temp.cache_size=-8192; \
         PRAGMA temp.max_page_count={pages}; PRAGMA main.max_page_count={pages};"
    ))?;
    let binding = Binding {
        source: Source {
            store_id: request.store_id,
            log_epoch: request.log_epoch,
        },
        watermark_tx: request.watermark_tx,
        graph: request.graph,
        shapes_hash: request.shapes_hash,
    };
    let limits = Limits {
        frame_bytes: crate::feed_parts::MAX_FRAME_BYTES,
        total_bytes: request.max_input_bytes,
        frames: request.max_frames,
    };
    let mut output_bytes = 0u64;
    let receipt = convert(
        &mut store,
        &binding,
        &limits,
        Frames {
            reader: BufReader::new(input),
            deadline,
        },
        |index, data| {
            output_bytes = output_bytes
                .checked_add(data.len() as u64)
                .ok_or_else(|| invalid("as-of RDF output count overflow"))?;
            if Instant::now() >= deadline
                || usage(scratch, input_bytes, output_bytes, temp_reserve)? > request.max_disk_bytes
            {
                return Err(invalid("as-of RDF disk or wall budget exceeded"));
            }
            let mut file = create(&scratch.join(format!("chunk-{index:08}.ttl")))?;
            file.write_all(data)
                .and_then(|()| file.sync_all())
                .map_err(|_| invalid("as-of RDF chunk write failed"))
        },
    )?;
    let complete = serde_json::to_vec(&json!({
        "schema":"quipu-asof-rdf/v1","source":{"store_id":binding.source.store_id,
        "log_epoch":binding.source.log_epoch},"graph":binding.graph,
        "shapes_hash":binding.shapes_hash,"receipt":receipt,
    }))
    .map_err(|_| invalid("as-of RDF receipt encoding failed"))?;
    if Instant::now() >= deadline
        || usage(scratch, input_bytes, output_bytes, temp_reserve)?
            .checked_add(complete.len() as u64)
            .is_none_or(|n| n > request.max_disk_bytes)
    {
        return Err(invalid("as-of RDF final resource bound exceeded"));
    }
    let mut file = create(&scratch.join("complete.json"))?;
    file.write_all(&complete)
        .and_then(|()| file.sync_all())
        .map_err(|_| invalid("as-of RDF completion write failed"))?;
    Ok(receipt)
}
