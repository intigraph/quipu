//! One pre-session bootstrap with explicit checksum-pinned GPU acquisition.
mod gpu;
pub use gpu::{acquire_gpu_candidate, gpu_candidate, provision_gpu_candidate};

use serde::Serialize;
#[cfg(feature = "bundled-onnx")]
use std::io::{Read, Write};
use std::{
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};

/// The loaded native runtime; model and execution-provider readiness are separate.
#[derive(Clone, Debug, Serialize)]
pub struct RuntimeInfo {
    /// True only after the native library loaded and passed ORT ABI checks.
    pub available: bool,
    /// Selected linkage and source, not a claim that a GPU session ran.
    pub linkage: String,
    /// Selected library path; a system-loader soname may be relative.
    pub path: PathBuf,
}

struct Initialized {
    info: RuntimeInfo,
    requested: PathBuf,
}
static RUNTIME: OnceLock<Initialized> = OnceLock::new();
static INITIALIZE: Mutex<()> = Mutex::new(());

/// Inspect the selected core without initializing or changing device selection.
pub fn current() -> Option<RuntimeInfo> {
    RUNTIME.get().map(|state| state.info.clone())
}

/// Call AFTER device selection and BEFORE any other ORT API.
/// Explicit `ORT_DYLIB_PATH` wins, then the caller's verified device core, then
/// the compiled CPU bundle. A failed explicit selection never falls back.
pub fn initialize(selected: Option<&Path>) -> crate::Result<RuntimeInfo> {
    let override_path = std::env::var_os("ORT_DYLIB_PATH");
    let explicit = override_path.is_some();
    let (path, linkage) = if let Some(path) = override_path {
        if path.is_empty() {
            return Err(crate::Error::Store("ORT_DYLIB_PATH is empty".into()));
        }
        (PathBuf::from(path), "dynamic-explicit")
    } else if let Some(path) = selected {
        (path.to_path_buf(), "dynamic-selected")
    } else {
        #[cfg(feature = "bundled-onnx")]
        {
            default_core()?
        }
        #[cfg(not(feature = "bundled-onnx"))]
        {
            default_core()
        }
    };
    let _guard = INITIALIZE
        .lock()
        .map_err(|_| crate::Error::Store("runtime initialization lock failed".into()))?;
    if let Some(state) = RUNTIME.get() {
        return if state.info.path == path || state.requested == path {
            Ok(state.info.clone())
        } else {
            Err(crate::Error::Store("ONNX Runtime already initialized with a different core; choose the device before initialization".into()))
        };
    }
    let requested = path.clone();
    let path = if !explicit && selected.is_some() {
        gpu::prepare_selected(&path)?
    } else {
        path
    };
    // A failed load does not seal our state: the device selector may explicitly
    // choose CPU after a GPU core failed before initialization. This function
    // never makes that fallback decision, especially for an explicit override.
    if !ort::init_from(&path)
        .map_err(|error| crate::Error::Store(format!("ONNX Runtime load failed: {error}")))?
        .commit()
    {
        return Err(crate::Error::Store("ONNX Runtime was configured outside the bootstrap; runtime selection cannot be attested".into()));
    }
    let info = RuntimeInfo {
        available: true,
        linkage: linkage.into(),
        path,
    };
    RUNTIME
        .set(Initialized {
            info: info.clone(),
            requested,
        })
        .map_err(|_| crate::Error::Store("runtime initialization raced".into()))?;
    Ok(info)
}

#[cfg(not(feature = "bundled-onnx"))]
fn default_core() -> (PathBuf, &'static str) {
    let name = if cfg!(target_os = "windows") {
        "onnxruntime.dll"
    } else if cfg!(target_os = "macos") {
        "libonnxruntime.dylib"
    } else {
        "libonnxruntime.so"
    };
    (name.into(), "dynamic-system")
}

#[cfg(feature = "bundled-onnx")]
fn default_core() -> crate::Result<(PathBuf, &'static str)> {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("LOCALAPPDATA").map(PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
        .ok_or_else(|| {
            crate::Error::Store("no private cache location for bundled ONNX Runtime".into())
        })?;
    let directory = base
        .join("quipu/runtime/1.23.2")
        .join(env!("QUIPU_ORT_CORE_SHA256"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&directory)
            .map_err(io_error)?;
    }
    #[cfg(not(unix))]
    std::fs::create_dir_all(&directory).map_err(io_error)?;
    let metadata = std::fs::symlink_metadata(&directory).map_err(io_error)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(crate::Error::Store(
            "runtime cache is not a private directory".into(),
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err(crate::Error::Store(
                "runtime cache directory must be private (0700)".into(),
            ));
        }
    }
    let destination = directory.join(env!("QUIPU_ORT_CORE_NAME"));
    let expected: usize = env!("QUIPU_ORT_CORE_SIZE")
        .parse()
        .expect("compiled runtime size");
    if !destination.try_exists().map_err(io_error)? {
        let mut bytes = Vec::new();
        flate2::read::GzDecoder::new(
            &include_bytes!(concat!(env!("OUT_DIR"), "/onnxruntime.gz"))[..],
        )
        .take(expected as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(io_error)?;
        verify_core(&bytes, expected)?;
        let mut temporary = tempfile::NamedTempFile::new_in(&directory).map_err(io_error)?;
        temporary.write_all(&bytes).map_err(io_error)?;
        temporary.as_file().sync_all().map_err(io_error)?;
        if let Err(error) = temporary.persist_noclobber(&destination)
            && error.error.kind() != std::io::ErrorKind::AlreadyExists
        {
            return Err(io_error(error.error));
        }
    }
    let metadata = std::fs::symlink_metadata(&destination).map_err(io_error)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() != expected as u64
    {
        return Err(crate::Error::Store(
            "bundled runtime cache identity is invalid".into(),
        ));
    }
    let bytes = std::fs::read(&destination).map_err(io_error)?;
    verify_core(&bytes, expected)?;
    // Redistribution notices are retained with the materialized library.
    retain_notice(
        &directory,
        "LICENSE",
        include_bytes!(concat!(env!("OUT_DIR"), "/onnxruntime.LICENSE")),
    )?;
    retain_notice(
        &directory,
        "ThirdPartyNotices.txt",
        include_bytes!(concat!(env!("OUT_DIR"), "/onnxruntime.NOTICES")),
    )?;
    Ok((destination, "bundled-dynamic-cpu"))
}

#[cfg(feature = "bundled-onnx")]
fn retain_notice(directory: &Path, name: &str, bytes: &[u8]) -> crate::Result<()> {
    let path = directory.join(name);
    if !path.try_exists().map_err(io_error)? {
        let mut temporary = tempfile::NamedTempFile::new_in(directory).map_err(io_error)?;
        temporary.write_all(bytes).map_err(io_error)?;
        if let Err(error) = temporary.persist_noclobber(&path)
            && error.error.kind() != std::io::ErrorKind::AlreadyExists
        {
            return Err(io_error(error.error));
        }
    }
    let metadata = std::fs::symlink_metadata(&path).map_err(io_error)?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() != bytes.len() as u64
        || std::fs::read(path).map_err(io_error)? != bytes
    {
        return Err(crate::Error::Store(
            "runtime redistribution notice identity is invalid".into(),
        ));
    }
    Ok(())
}

#[cfg(feature = "bundled-onnx")]
fn verify_core(bytes: &[u8], expected: usize) -> crate::Result<()> {
    let digest = hex::encode(ring::digest::digest(&ring::digest::SHA256, bytes));
    if bytes.len() != expected || digest != env!("QUIPU_ORT_CORE_SHA256") {
        return Err(crate::Error::Store(
            "bundled ONNX Runtime checksum mismatch; cache was not loaded".into(),
        ));
    }
    Ok(())
}

#[cfg(feature = "bundled-onnx")]
// map_err and tempfile persistence pass owned errors to this adapter.
#[allow(clippy::needless_pass_by_value)]
fn io_error(error: std::io::Error) -> crate::Error {
    crate::Error::Store(format!("bundled ONNX Runtime cache failed: {error}"))
}
