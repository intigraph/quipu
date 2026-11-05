//! Explicit, pinned model installation. Ordinary commands never download.
use std::path::{Path, PathBuf};

use crate::{EmbeddingConfig, Error, Result};

/// Immutable upstream revision; updates require reviewing both hashes.
pub const REVISION: &str = "1110a243fdf4706b3f48f1d95db1a4f5529b4d41";
const MODEL_HASH: &str = "6fd5d72fe4589f189f8ebc006442dbb529bb7ce38f8082112682524616046452";
const TOKENIZER_HASH: &str = "be50c3628f2bf5bb5e3a7f17b1f74611b2561a3a27eeab05e5aa30f411572037";
const ASSETS: [(&str, &str, &str, u64); 2] = [
    ("model.onnx", "onnx/model.onnx", MODEL_HASH, 90_405_214),
    ("tokenizer.json", "tokenizer.json", TOKENIZER_HASH, 466_247),
];

fn io<T>(value: std::io::Result<T>) -> Result<T> {
    value.map_err(|error| Error::Store(format!("model assets: {error}")))
}

fn invalid(message: impl Into<String>) -> Error {
    Error::InvalidValue(message.into())
}

/// Versioned cache location, respecting the XDG data directory.
pub fn default_directory() -> Result<PathBuf> {
    let data = std::env::var_os("XDG_DATA_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("LOCALAPPDATA")
                .filter(|v| !v.is_empty())
                .map(PathBuf::from)
        })
        .or_else(|| {
            std::env::var_os("HOME")
                .filter(|v| !v.is_empty())
                .map(|v| PathBuf::from(v).join(".local/share"))
        })
        .ok_or_else(|| invalid("model cache requires HOME or XDG_DATA_HOME"))?;
    if !data.is_absolute() {
        return Err(invalid("model data directory must be absolute"));
    }
    Ok(data
        .join("quipu/models")
        .join(format!("all-MiniLM-L6-v2-{REVISION}")))
}

fn verify(directory: &Path) -> Result<()> {
    use std::io::Read;
    for (name, _, expected, length) in ASSETS {
        let mut file = io(std::fs::File::open(directory.join(name)))?;
        if io(file.metadata())?.len() != length {
            return Err(invalid(format!(
                "installed {name} has the wrong size; preserve the cache and reinstall to a clean location"
            )));
        }
        let mut digest = ring::digest::Context::new(&ring::digest::SHA256);
        let mut buffer = [0; 65536];
        loop {
            let n = io(file.read(&mut buffer))?;
            if n == 0 {
                break;
            }
            digest.update(&buffer[..n]);
        }
        if hex::encode(digest.finish().as_ref()) != expected {
            return Err(invalid(format!("installed {name} checksum mismatch")));
        }
    }
    Ok(())
}

/// Resolve an installed default only when neither explicit path is configured.
/// Explicit paths remain authoritative; `auto_embed` only controls vector writes.
pub fn resolve(config: &EmbeddingConfig) -> Result<EmbeddingConfig> {
    let mut resolved = config.clone();
    if config.model_path.is_some() || config.tokenizer_path.is_some() {
        return Ok(resolved);
    }
    let Ok(directory) = default_directory() else {
        return Ok(resolved);
    };
    if !io(directory.try_exists())? {
        return Ok(resolved);
    }
    verify(&directory)?;
    resolved.model_path = Some(directory.join("model.onnx"));
    resolved.tokenizer_path = Some(directory.join("tokenizer.json"));
    resolved.dimension = 384;
    Ok(resolved)
}

/// Fetch exactly the pinned files and atomically publish a verified directory.
/// No database is opened or backfilled by this operation.
#[cfg(feature = "remote")]
pub fn install(directory: &Path) -> Result<()> {
    use std::io::{Read, Write};
    if io(directory.try_exists())? {
        return verify(directory);
    }
    let parent = directory
        .parent()
        .ok_or_else(|| invalid("model cache needs a parent directory"))?;
    io(std::fs::create_dir_all(parent))?;
    let staging = io(tempfile::tempdir_in(parent))?;
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(std::time::Duration::from_secs(20))
        .timeout_read(std::time::Duration::from_secs(120))
        .timeout(std::time::Duration::from_secs(300))
        .build();
    for (name, remote, _, length) in ASSETS {
        let url = format!(
            "https://huggingface.co/sentence-transformers/all-MiniLM-L6-v2/resolve/{REVISION}/{remote}"
        );
        let response = agent
            .get(&url)
            .call()
            .map_err(|e| invalid(format!("model download failed: {e}")))?;
        let mut input = response.into_reader().take(length + 1);
        let mut file = io(std::fs::File::create(staging.path().join(name)))?;
        let written = io(std::io::copy(&mut input, &mut file))?;
        if written != length {
            return Err(invalid(format!("{name} download length mismatch")));
        }
        io(file.flush())?;
        io(file.sync_all())?;
    }
    verify(staging.path())?;
    // A concurrent installer wins only with the same verified pinned bytes.
    if io(directory.try_exists())? {
        return verify(directory);
    }
    io(std::fs::rename(staging.path(), directory))?;
    Ok(())
}

/// Builds without the network client retain offline search and a clear remedy.
#[cfg(not(feature = "remote"))]
pub fn install(_directory: &Path) -> Result<()> {
    Err(invalid(
        "model install requires the remote feature; install Quipu with --features full",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_paths_do_not_consult_default_cache() {
        let config = EmbeddingConfig {
            model_path: Some("explicit-model.onnx".into()),
            ..EmbeddingConfig::default()
        };
        let resolved = resolve(&config).unwrap();
        assert_eq!(resolved.model_path, config.model_path);
        assert!(resolved.tokenizer_path.is_none());
    }

    #[test]
    fn corrupt_cache_is_refused_without_overwriting_it() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("model.onnx");
        std::fs::write(&path, b"corrupt").unwrap();
        assert!(verify(directory.path()).is_err());
        assert_eq!(std::fs::read(path).unwrap(), b"corrupt");
    }
}
