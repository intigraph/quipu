//! Atomic publication within an explicitly opened destination parent capability.
//! Rendering owns the complete root; it never adopts unrelated entries.

use super::{MAX_BYTES, MAX_FILES, RenderFile, RenderPlan, check_path, invalid};
use crate::Result;
use base64::{Engine, engine::general_purpose::STANDARD};
use cap_std::fs::{Dir, Metadata, MetadataExt};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

const MANIFEST: &str = ".quipu-render.json";
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Stamp {
    pub(crate) sha256: String,
    pub(crate) mode: u32,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema: u32,
    spec: String,
    source_tx: i64,
    valid_at: Option<String>,
    files: BTreeMap<String, Stamp>,
    // Last successful publication event. A no-op never rewrites it. Wall
    // clock is not embedded in the deterministic documents.
    rendered_at: String,
}
pub use super::diff::Changes;
fn io(error: impl std::fmt::Display) -> crate::Error {
    invalid(format!("filesystem: {error}"))
}
fn stamps(files: &BTreeMap<String, RenderFile>) -> BTreeMap<String, Stamp> {
    files
        .iter()
        .map(|(path, file)| {
            (
                path.clone(),
                Stamp {
                    sha256: file.sha256.clone(),
                    mode: file.mode,
                },
            )
        })
        .collect()
}
#[cfg(unix)]
fn open_parent(path: &Path) -> Result<Dir> {
    // Walk from the filesystem root through directory capabilities. Each
    // component is opened NOFOLLOW; no checked pathname is reopened ambiently.
    let mut dir = Dir::open_ambient_dir("/", cap_std::ambient_authority()).map_err(io)?;
    for component in path.components() {
        match component {
            std::path::Component::RootDir => {}
            std::path::Component::Normal(name) => dir = open_root(&dir, Path::new(name))?,
            _ => return Err(invalid("parent must have canonical directory components")),
        }
    }
    Ok(dir)
}
#[cfg(not(unix))]
fn open_parent(_: &Path) -> Result<Dir> {
    Err(invalid("publication requires Unix directory capabilities"))
}
#[cfg(unix)]
fn mode(m: &Metadata) -> u32 {
    m.mode() & 0o7777
}
#[cfg(not(unix))]
fn mode(_: &Metadata) -> u32 {
    0o644
}

fn inventory(
    root: &Dir,
    relative: &Path,
    out: &mut BTreeMap<String, Stamp>,
    total: &mut usize,
) -> Result<()> {
    let reader = if relative.as_os_str().is_empty() {
        root.entries()
    } else {
        root.read_dir(relative)
    };
    let mut entries = reader
        .map_err(io)?
        .collect::<std::io::Result<Vec<_>>>()
        .map_err(io)?;
    entries.sort_by_key(cap_std::fs::DirEntry::file_name);
    for entry in entries {
        let rel = relative.join(entry.file_name());
        let key = rel
            .to_str()
            .ok_or_else(|| invalid("non-UTF8 destination entry"))?
            .to_string();
        let m = root.symlink_metadata(&rel).map_err(io)?;
        if m.file_type().is_dir() {
            if root.read_dir(&rel).map_err(io)?.next().is_none() {
                return Err(invalid(format!("unmanaged empty directory: {key}")));
            }
            inventory(root, &rel, out, total)?;
        } else if m.file_type().is_file() {
            if relative.as_os_str().is_empty() && key == MANIFEST {
                continue;
            }
            check_path(&key)?;
            *total = total.saturating_add(m.len() as usize);
            if out.len() >= MAX_FILES || *total > MAX_BYTES {
                return Err(invalid("destination cap exceeded"));
            }
            let data = root.read(&rel).map_err(io)?;
            out.insert(
                key,
                Stamp {
                    sha256: crate::share::sha256(&data),
                    mode: mode(&m),
                },
            );
        } else {
            return Err(invalid(format!(
                "symlink or special destination entry: {key}"
            )));
        }
    }
    Ok(())
}
struct Current {
    dir: Option<Dir>,
    manifest: Option<Manifest>,
    files: BTreeMap<String, Stamp>,
}
#[cfg(unix)]
fn open_root(parent: &Dir, name: &Path) -> Result<Dir> {
    let fd = rustix::fs::openat(
        parent,
        name,
        rustix::fs::OFlags::RDONLY
            | rustix::fs::OFlags::DIRECTORY
            | rustix::fs::OFlags::NOFOLLOW
            | rustix::fs::OFlags::CLOEXEC,
        rustix::fs::Mode::empty(),
    )
    .map_err(io)?;
    Ok(Dir::from_std_file(fs::File::from(fd)))
}
#[cfg(not(unix))]
fn open_root(parent: &Dir, name: &Path) -> Result<Dir> {
    parent.open_dir(name).map_err(io)
}
fn current(parent: &Dir, name: &Path) -> Result<Current> {
    match parent.symlink_metadata(name) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Current {
                dir: None,
                manifest: None,
                files: BTreeMap::new(),
            });
        }
        Err(e) => return Err(io(e)),
        Ok(m) if !m.file_type().is_dir() => {
            return Err(invalid("destination must be a real directory"));
        }
        Ok(_) => {}
    }
    let root = open_root(parent, name)?;
    let mut actual = BTreeMap::new();
    inventory(&root, Path::new(""), &mut actual, &mut 0)?;
    let manifest = match root.symlink_metadata(MANIFEST) {
        Ok(m) => {
            if !m.file_type().is_file() || m.len() > MAX_BYTES as u64 {
                return Err(invalid("invalid manifest file"));
            }
            let manifest: Manifest = serde_json::from_slice(&root.read(MANIFEST).map_err(io)?)
                .map_err(|e| invalid(format!("invalid manifest: {e}")))?;
            if manifest.schema != 1 {
                return Err(invalid("unknown manifest schema"));
            }
            for key in manifest.files.keys() {
                check_path(key)?;
            }
            if manifest.files != actual {
                return Err(invalid(
                    "managed bytes/modes edited, missing, or unknown files present",
                ));
            }
            Some(manifest)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            if !actual.is_empty() {
                return Err(invalid("unmanaged destination files present"));
            }
            None
        }
        Err(e) => return Err(io(e)),
    };
    Ok(Current {
        dir: Some(root),
        manifest,
        files: actual,
    })
}
fn same(a: &Current, b: &Current) -> Result<bool> {
    if a.manifest != b.manifest || a.files != b.files {
        return Ok(false);
    }
    match (&a.dir, &b.dir) {
        (None, None) => Ok(true),
        (Some(a), Some(b)) => {
            #[cfg(unix)]
            {
                let a = a.dir_metadata().map_err(io)?;
                let b = b.dir_metadata().map_err(io)?;
                Ok(a.dev() == b.dev() && a.ino() == b.ino())
            }
            #[cfg(not(unix))]
            {
                Ok(false)
            }
        }
        _ => Ok(false),
    }
}

/// Dry-run and no-op perform zero filesystem writes. New publication requires
/// an existing parent and atomically exchanges the complete owned root.
pub fn publish(plan: &RenderPlan, destination: &Path, dry_run: bool) -> Result<Changes> {
    plan.validate()?;
    if destination.components().any(|c| {
        matches!(
            c,
            std::path::Component::ParentDir | std::path::Component::CurDir
        )
    }) {
        return Err(invalid("destination must not contain dot components"));
    }
    let root = if destination.is_absolute() {
        destination.to_path_buf()
    } else {
        std::env::current_dir().map_err(io)?.join(destination)
    };
    let parent_path = root
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .ok_or_else(|| invalid("destination cannot be filesystem root"))?;
    let name = root
        .file_name()
        .ok_or_else(|| invalid("destination requires a directory name"))?;
    let parent = open_parent(parent_path)?;
    #[cfg(unix)]
    let lock = fs::File::from(
        rustix::fs::openat(
            &parent,
            ".",
            rustix::fs::OFlags::RDONLY
                | rustix::fs::OFlags::DIRECTORY
                | rustix::fs::OFlags::CLOEXEC,
            rustix::fs::Mode::empty(),
        )
        .map_err(io)?,
    );
    #[cfg(unix)]
    rustix::fs::flock(&lock, rustix::fs::FlockOperation::NonBlockingLockExclusive).map_err(
        |e| {
            invalid(format!(
                "destination parent lock refused: {e}; no publication started"
            ))
        },
    )?;
    let before = current(&parent, Path::new(name))?;
    let desired = stamps(&plan.files);
    let noop = before.manifest.as_ref().is_some_and(|m| {
        m.files == desired
            && m.spec == plan.spec
            && m.source_tx == plan.source_tx
            && m.valid_at == plan.valid_at
    });
    let result = super::diff::differences(plan, &before.files, before.dir.as_ref(), noop)?;
    if dry_run || result.noop {
        return Ok(result);
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    return Err(invalid(
        "atomic publication supported only on Linux and macOS",
    ));
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        let mut stage = Stage::new(&parent)?;
        for (path, file) in &plan.files {
            let path = Path::new(path);
            stage
                .dir
                .create_dir_all(path.parent().expect("relative file parent"))
                .map_err(io)?;
            if before.files.get(path.to_str().expect("UTF8 plan"))
                == desired.get(path.to_str().expect("UTF8 plan"))
            {
                before
                    .dir
                    .as_ref()
                    .expect("unchanged file has a root")
                    .hard_link(path, &stage.dir, path)
                    .map_err(io)?;
            } else {
                stage
                    .dir
                    .write(path, STANDARD.decode(&file.base64).map_err(io)?)
                    .map_err(io)?;
                use cap_std::fs::PermissionsExt;
                stage
                    .dir
                    .set_permissions(path, cap_std::fs::Permissions::from_mode(file.mode))
                    .map_err(io)?;
            }
            stage.dir.open(path).map_err(io)?.sync_all().map_err(io)?;
        }
        let manifest = Manifest {
            schema: 1,
            spec: plan.spec.clone(),
            source_tx: plan.source_tx,
            valid_at: plan.valid_at.clone(),
            files: desired,
            rendered_at: crate::time::now_iso(),
        };
        stage
            .dir
            .write(MANIFEST, serde_json::to_vec_pretty(&manifest).map_err(io)?)
            .map_err(io)?;
        use cap_std::fs::PermissionsExt;
        stage
            .dir
            .set_permissions(MANIFEST, cap_std::fs::Permissions::from_mode(0o600))
            .map_err(io)?;
        let root_mode = before
            .dir
            .as_ref()
            .map(|dir| dir.dir_metadata().map(|m| mode(&m)))
            .transpose()
            .map_err(io)?
            .unwrap_or(0o700);
        stage
            .dir
            .set_permissions(".", cap_std::fs::Permissions::from_mode(root_mode))
            .map_err(io)?;
        stage
            .dir
            .open(MANIFEST)
            .map_err(io)?
            .sync_all()
            .map_err(io)?;
        stage
            .dir
            .try_clone()
            .map_err(io)?
            .into_std_file()
            .sync_all()
            .map_err(io)?;
        if !same(&before, &current(&parent, Path::new(name))?)? {
            return Err(invalid("destination changed during staging"));
        }
        swap(&parent, &stage.name, Path::new(name), before.dir.is_some())?;
        stage.cleanup = false;
        if before.dir.is_some() && !same(&before, &current(&parent, &stage.name)?)? {
            // Preserve BOTH directories if an uncooperative writer changed the
            // old root. Do not delete its data or blindly overwrite a new root.
            return Err(invalid(format!(
                "publication UNKNOWN: old destination changed; retained {}",
                stage.name.display()
            )));
        }
        lock.sync_all().map_err(io)?;
        stage.cleanup = true;
        Ok(result)
    }
}
#[cfg(any(target_os = "linux", target_os = "macos"))]
struct Stage {
    parent: Dir,
    name: PathBuf,
    dir: Dir,
    cleanup: bool,
}
#[cfg(any(target_os = "linux", target_os = "macos"))]
impl Stage {
    fn new(parent: &Dir) -> Result<Self> {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNT: AtomicU64 = AtomicU64::new(0);
        let name = PathBuf::from(format!(
            ".quipu-render-stage-{}-{}",
            std::process::id(),
            COUNT.fetch_add(1, Ordering::Relaxed)
        ));
        rustix::fs::mkdirat(parent, &name, rustix::fs::Mode::from_bits_truncate(0o700))
            .map_err(io)?;
        let dir = open_root(parent, &name)?;
        Ok(Self {
            parent: parent.try_clone().map_err(io)?,
            name,
            dir,
            cleanup: true,
        })
    }
}
#[cfg(any(target_os = "linux", target_os = "macos"))]
impl Drop for Stage {
    fn drop(&mut self) {
        if self.cleanup {
            let _ = self.parent.remove_dir_all(&self.name);
        }
    }
}
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn swap(parent: &Dir, stage: &Path, root: &Path, existing: bool) -> Result<()> {
    let flags = if existing {
        rustix::fs::RenameFlags::EXCHANGE
    } else {
        rustix::fs::RenameFlags::NOREPLACE
    };
    rustix::fs::renameat_with(parent, stage, parent, root, flags).map_err(io)
}
