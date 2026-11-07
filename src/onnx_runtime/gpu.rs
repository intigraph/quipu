//! Optional matched GPU bundle, reusing Bobbin's pinned userspace ABI.
//! Discovery never downloads, loads libraries, or initializes ORT. Device policy
//! remains with the caller. Only an explicitly selected candidate is staged.
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

const VERSION: &str = "onnx-1.23.2-cuda12-v1";
const CORE: &str = "libonnxruntime.so.1.23.2";
const MAX_FILE: u64 = 1024 * 1024 * 1024;

#[derive(Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Receipt {
    version: String,
    files: BTreeMap<String, String>,
}

fn error(message: impl std::fmt::Display) -> crate::Error {
    crate::Error::Store(format!("matched GPU runtime: {message}"))
}

fn approved() -> Receipt {
    serde_json::from_str(include_str!("gpu-files.json")).expect("compiled GPU catalogue")
}

fn regular(path: &Path) -> crate::Result<File> {
    let metadata = fs::symlink_metadata(path).map_err(error)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > MAX_FILE {
        return Err(error("bounded regular non-symlink file required"));
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    options.open(path).map_err(error)
}

fn digest_copy(mut source: File, mut output: Option<&mut File>) -> crate::Result<String> {
    let mut digest = ring::digest::Context::new(&ring::digest::SHA256);
    let mut buffer = [0u8; 128 * 1024];
    let mut total = 0u64;
    loop {
        let count = source.read(&mut buffer).map_err(error)?;
        if count == 0 {
            break;
        }
        total += count as u64;
        if total > MAX_FILE {
            return Err(error("library exceeds byte budget"));
        }
        digest.update(&buffer[..count]);
        if let Some(file) = output.as_mut() {
            file.write_all(&buffer[..count]).map_err(error)?;
        }
    }
    Ok(hex::encode(digest.finish()))
}

fn present(path: &Path) -> crate::Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(failure) if failure.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(failure) => Err(error(failure)),
    }
}

fn directory(path: &Path) -> crate::Result<()> {
    let metadata = fs::symlink_metadata(path).map_err(error)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(error("non-symlink bundle directory required"));
    }
    Ok(())
}

fn verify(root: &Path, expected: &Receipt) -> crate::Result<PathBuf> {
    directory(root)?;
    directory(&root.join("lib"))?;
    let receipt: Receipt =
        serde_json::from_reader(regular(&root.join("receipt.json"))?.take(16385)).map_err(error)?;
    if receipt != *expected || receipt.version != VERSION {
        return Err(error("receipt differs from compiled matched bundle pins"));
    }
    for (name, digest) in &expected.files {
        if Path::new(name).file_name().and_then(|s| s.to_str()) != Some(name) {
            return Err(error("invalid compiled library name"));
        }
        if digest_copy(regular(&root.join("lib").join(name))?, None)? != *digest {
            return Err(error(format!("checksum mismatch: {name}")));
        }
    }
    Ok(root.join("lib").join(CORE))
}

/// Discover a verified optional GPU core without fetching or initializing ORT.
/// Explicit ORT overrides bypass discovery, even when invalid. Linux `x86_64`
/// uses `QUIPU_GPU_CACHE`, then `BOBBIN_GPU_CACHE`, then Bobbin's XDG data contract.
/// An absent cache yields None; malformed or changed bytes remain an error so
/// the device selector can report an explicit CPU fallback reason.
pub fn gpu_candidate() -> crate::Result<Option<PathBuf>> {
    if std::env::var_os("ORT_DYLIB_PATH").is_some()
        || std::env::var("QUIPU_GPU").is_ok_and(|v| v == "0" || v.eq_ignore_ascii_case("false"))
        || !cfg!(all(target_os = "linux", target_arch = "x86_64"))
    {
        return Ok(None);
    }
    if let Some(root) = std::env::var_os("QUIPU_GPU_CACHE") {
        let root = PathBuf::from(root).join(VERSION);
        return if present(&root)? {
            verify(&root, &approved()).map(Some)
        } else {
            Ok(None)
        };
    }
    let owned = owned_cache()?.join(VERSION);
    if present(&owned)? {
        return verify(&owned, &approved()).map(Some);
    }
    let root = std::env::var_os("BOBBIN_GPU_CACHE")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("XDG_DATA_HOME")
                .map(PathBuf::from)
                .or_else(|| std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".local/share")))
                .map(|p| p.join("bobbin/gpu-runtime"))
        })
        .ok_or_else(|| error("no GPU cache location"))?
        .join(VERSION);
    if !present(&root)? {
        return Ok(None);
    }
    verify(&root, &approved()).map(Some)
}

/// Stage an existing matched bundle into a private content-pinned directory.
/// This is offline provisioning, not a second network fetcher. No library is
/// loaded; a CPU decision can still be made after this operation.
pub fn provision_gpu_candidate(source: &Path, cache: &Path) -> crate::Result<PathBuf> {
    provision(source, cache, &approved())
}

fn provision(source: &Path, cache: &Path, expected: &Receipt) -> crate::Result<PathBuf> {
    verify(source, expected)?;
    private_cache(cache)?;
    let destination = cache.join(VERSION);
    if present(&destination)? {
        return verify(&destination, expected);
    }
    let stage = tempfile::Builder::new()
        .prefix(".gpu-stage-")
        .tempdir_in(cache)
        .map_err(error)?;
    fs::create_dir(stage.path().join("lib")).map_err(error)?;
    for (name, digest) in &expected.files {
        let path = stage.path().join("lib").join(name);
        let mut output = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&path)
            .map_err(error)?;
        if digest_copy(regular(&source.join("lib").join(name))?, Some(&mut output))? != *digest {
            return Err(error(format!("source changed during staging: {name}")));
        }
        output.sync_all().map_err(error)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(path, fs::Permissions::from_mode(0o400)).map_err(error)?;
        }
    }
    let notices = source.join("licenses");
    directory(&notices)?;
    fs::create_dir(stage.path().join("licenses")).map_err(error)?;
    let entries: Vec<_> = fs::read_dir(&notices).map_err(error)?.take(257).collect();
    if entries.len() > 256 {
        return Err(error("GPU license count budget exceeded"));
    }
    for entry in entries {
        let entry = entry.map_err(error)?;
        let mut bytes = Vec::new();
        regular(&entry.path())?
            .take(16 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(error)?;
        if bytes.len() > 16 * 1024 * 1024 {
            return Err(error("GPU license byte budget exceeded"));
        }
        fs::write(stage.path().join("licenses").join(entry.file_name()), bytes).map_err(error)?;
    }
    let bytes =
        serde_json::to_vec(&serde_json::json!({"version":expected.version,"files":expected.files}))
            .map_err(error)?;
    fs::write(stage.path().join("receipt.json"), bytes).map_err(error)?;
    verify(stage.path(), expected)?;
    // A concurrent successful publisher is reused; a corrupt prior cache is
    // retained and refused, never silently repaired or overwritten.
    if let Err(failure) = fs::rename(stage.path(), &destination)
        && !present(&destination)?
    {
        return Err(error(failure));
    }
    verify(&destination, expected)
}

#[cfg(test)]
mod tests;

fn owned_cache() -> crate::Result<PathBuf> {
    std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".cache")))
        .map(|p| p.join("quipu/gpu-runtime"))
        .ok_or_else(|| error("no private GPU cache location"))
}

/// Acquire the optional pinned GPU bundle AFTER device/hold policy permits it.
/// Reuses a verified existing candidate before downloading the same seven pinned
/// artifacts as Bobbin. Never call for CPU, API/remote or held paths. Errors are
/// returned to the device selector for an explicit CPU fallback reason.
/// No driver, package-manager action, ORT initialization or process restart occurs.
pub fn acquire_gpu_candidate() -> crate::Result<Option<PathBuf>> {
    if let Some(core) = gpu_candidate()? {
        return Ok(Some(core));
    }
    if std::env::var_os("ORT_DYLIB_PATH").is_some()
        || std::env::var("QUIPU_GPU").is_ok_and(|v| v == "0" || v.eq_ignore_ascii_case("false"))
        || !cfg!(all(target_os = "linux", target_arch = "x86_64"))
    {
        return Ok(None);
    }
    acquire(&owned_cache()?, &approved()).map(Some)
}

#[derive(Deserialize)]
struct Artifact {
    name: String,
    url: String,
    sha256: String,
}

fn acquire(cache: &Path, expected: &Receipt) -> crate::Result<PathBuf> {
    let agent = ureq::AgentBuilder::new()
        .https_only(true)
        .timeout_connect(std::time::Duration::from_secs(20))
        .timeout(std::time::Duration::from_secs(900))
        .build();
    let artifacts: Vec<Artifact> =
        serde_json::from_str(include_str!("gpu-artifacts.json")).map_err(error)?;
    acquire_from(cache, expected, &artifacts, |artifact, archive| {
        let mut response = agent
            .get(&artifact.url)
            .call()
            .map_err(|_| {
                error(format!(
                    "pinned GPU artifact unavailable: {}",
                    artifact.name
                ))
            })?
            .into_reader()
            .take(MAX_FILE + 1);
        if std::io::copy(&mut response, archive).map_err(error)? > MAX_FILE {
            return Err(error("GPU archive exceeds byte budget"));
        }
        Ok(())
    })
}

fn acquire_from(
    cache: &Path,
    expected: &Receipt,
    artifacts: &[Artifact],
    mut fetch: impl FnMut(&Artifact, &mut File) -> crate::Result<()>,
) -> crate::Result<PathBuf> {
    use std::io::{Seek, SeekFrom};
    private_cache(cache)?;
    let lock_path = cache.join("install.lock");
    let mut options = OpenOptions::new();
    options.create(true).truncate(false).read(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let lock = options.open(lock_path).map_err(error)?;
    #[cfg(unix)]
    rustix::fs::flock(&lock, rustix::fs::FlockOperation::NonBlockingLockExclusive)
        .map_err(|_| error("GPU acquisition already active; no competing download"))?;
    let destination = cache.join(VERSION);
    if present(&destination)? {
        return verify(&destination, expected);
    }
    let stage = tempfile::Builder::new()
        .prefix(".gpu-download-")
        .tempdir_in(cache)
        .map_err(error)?;
    fs::create_dir(stage.path().join("lib")).map_err(error)?;
    fs::create_dir(stage.path().join("licenses")).map_err(error)?;
    let mut total = 0u64;
    for artifact in artifacts {
        // URL and digest come only from the compiled, reviewed catalogue.
        if !artifact.url.starts_with("https://files.pythonhosted.org/") {
            return Err(error("invalid pinned artifact origin"));
        }
        let mut archive = tempfile::tempfile_in(cache).map_err(error)?;
        fetch(artifact, &mut archive)?;
        archive.seek(SeekFrom::Start(0)).map_err(error)?;
        if digest_copy(archive.try_clone().map_err(error)?, None)? != artifact.sha256 {
            return Err(error("GPU artifact checksum mismatch"));
        }
        archive.seek(SeekFrom::Start(0)).map_err(error)?;
        extract(archive, stage.path(), expected, &artifact.name, &mut total)?;
    }
    fs::write(
        stage.path().join("receipt.json"),
        serde_json::to_vec(&serde_json::json!({"version":expected.version,"files":expected.files}))
            .map_err(error)?,
    )
    .map_err(error)?;
    verify(stage.path(), expected)?;
    fs::rename(stage.path(), &destination).map_err(error)?;
    verify(&destination, expected)
}

fn private_cache(cache: &Path) -> crate::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, MetadataExt};
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(cache)
            .map_err(error)?;
        let metadata = fs::symlink_metadata(cache).map_err(error)?;
        if metadata.mode() & 0o077 != 0 || metadata.uid() != rustix::process::geteuid().as_raw() {
            return Err(error("GPU staging cache must be owned and private"));
        }
    }
    #[cfg(not(unix))]
    fs::create_dir_all(cache).map_err(error)?;
    directory(cache)
}

fn extract(
    archive: File,
    stage: &Path,
    expected: &Receipt,
    artifact: &str,
    total: &mut u64,
) -> crate::Result<()> {
    let mut zip = runtime_zip::ZipArchive::new(archive).map_err(error)?;
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index).map_err(error)?;
        let enclosed = entry
            .enclosed_name()
            .ok_or_else(|| error("unsafe GPU archive member"))?;
        let name = enclosed
            .file_name()
            .and_then(|p| p.to_str())
            .ok_or_else(|| error("invalid GPU member name"))?
            .to_string();
        if entry.is_dir() {
            continue;
        }
        if entry
            .unix_mode()
            .is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            return Err(error("GPU archive symlink refused"));
        }
        let library = expected.files.contains_key(&name);
        let license = name.to_ascii_lowercase().starts_with("license");
        if !library && !license {
            continue;
        }
        *total = total
            .checked_add(entry.size())
            .ok_or_else(|| error("GPU unpacked size overflow"))?;
        if *total > 6 * 1024 * 1024 * 1024 || entry.size() > MAX_FILE {
            return Err(error("GPU unpacked byte budget exceeded"));
        }
        let path = if library {
            stage.join("lib").join(&name)
        } else {
            stage
                .join("licenses")
                .join(format!("{artifact}-{index}-{name}"))
        };
        let mut output = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(path)
            .map_err(error)?;
        let count =
            std::io::copy(&mut entry.by_ref().take(MAX_FILE + 1), &mut output).map_err(error)?;
        if count != entry.size() {
            return Err(error("GPU member length mismatch"));
        }
        output.sync_all().map_err(error)?;
    }
    Ok(())
}

/// Prepare matched dependencies only for the caller-selected GPU core. All
/// files are privately staged and verified before dlopen. This is Linux-only;
/// explicit ORT overrides bypass it in `initialize()`. No ORT API is called here.
pub(super) fn prepare_selected(core: &Path) -> crate::Result<PathBuf> {
    if !cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        return Err(error("automatic GPU loader requires Linux x86_64"));
    }
    let root = core
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| error("GPU core has no bundle"))?;
    if core != root.join("lib").join(CORE) {
        return Err(error("selected core differs from matched bundle"));
    }
    let staged = provision_gpu_candidate(root, &owned_cache()?)?;
    #[cfg(target_os = "linux")]
    load_dependencies(staged.parent().expect("staged GPU lib directory"))?;
    Ok(staged)
}

#[cfg(target_os = "linux")]
#[allow(unsafe_code)] // dlopen is isolated here; only compiled-pinned staged bytes reach it.
fn load_dependencies(lib: &Path) -> crate::Result<()> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    // Preload only CUDA-provider DT_NEEDED userspace dependencies. The pinned
    // cuDNN components carry $ORIGIN RUNPATH and remain adjacent. Let ORT load
    // its providers after core initialization; loading them before their host
    // is initialized is outside their contract. No post-start LD path mutation.
    let mut pending: Vec<String> = [
        "libcublasLt.so.12",
        "libcublas.so.12",
        "libcudart.so.12",
        "libcudnn.so.9",
        "libcurand.so.10",
        "libnvJitLink.so.12",
        "libcufft.so.11",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    let mut handles = Vec::new();
    while !pending.is_empty() {
        let before = pending.len();
        pending.retain(|name| {
            let Ok(path) = CString::new(lib.join(name).as_os_str().as_bytes()) else {
                return true;
            };
            // SAFETY: path is NUL-terminated; flags request eager resolution of
            // verified libraries. Successful handles intentionally live until
            // process exit, like ORT itself; no dangling library pointers.
            let handle = unsafe { libc::dlopen(path.as_ptr(), libc::RTLD_NOW | libc::RTLD_GLOBAL) };
            if handle.is_null() {
                true
            } else {
                handles.push(handle);
                false
            }
        });
        if pending.len() == before {
            for handle in handles.into_iter().rev() {
                // SAFETY: each handle came from a successful dlopen in this call;
                // ORT has not initialized, so no retained ORT symbol uses it.
                unsafe { libc::dlclose(handle) };
            }
            return Err(error(format!(
                "CUDA dependency loader unavailable: {}",
                pending.join(",")
            )));
        }
    }
    Ok(())
}
