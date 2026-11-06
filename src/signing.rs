//! Quipu's application adapter to the published shared signing implementation.
//!
//! The shared crate owns v1 message bytes, Ed25519 encoding and host-file I/O.
//! Quipu retains its key-path convention and its registration/authorization
//! checks. Signature validity alone is never authorization. Replication and
//! session/share envelopes retain their own canonical domain-separated messages.

#[cfg(not(target_arch = "wasm32"))]
use crate::error::Result;
#[cfg(not(target_arch = "wasm32"))]
use ring::signature::Ed25519KeyPair;
#[cfg(not(target_arch = "wasm32"))]
use std::path::Path;

pub use quipu_signing::{SigningIdentity, public_key_hex, sign_hex, verdict_message, verify_hex};

/// Where the host-file signing key lives: `$QUIPU_SIGNING_KEY`, else
/// `$XDG_DATA_HOME/quipu/verifier.pk8`, else `$HOME/.local/share/quipu/verifier.pk8`.
///
/// Factored out because `quipu-server` and the `share --attest` CLI path both
/// need it, and two inline copies of a path convention drift silently -- the
/// failure being a signature minted with one key and verified against another.
#[cfg(not(target_arch = "wasm32"))]
pub fn default_key_path() -> Result<std::path::PathBuf> {
    if Path::new(".quipu/verifier.pk8").is_file() {
        eprintln!(
            "warning: legacy signing key in .quipu/verifier.pk8; pin QUIPU_SIGNING_KEY to retain that identity or migrate it explicitly; never commit the private key"
        );
    }
    let path = key_path(
        std::env::var_os("QUIPU_SIGNING_KEY").as_deref(),
        std::env::var_os("XDG_DATA_HOME").as_deref(),
        std::env::var_os("HOME").as_deref(),
    )?;
    protect_repository_key(&path)?;
    Ok(path)
}

/// Protect an explicitly selected legacy repository key before creating it.
/// The default key is outside the repository; this retains explicit overrides.
#[cfg(not(target_arch = "wasm32"))]
pub fn protect_repository_key(path: &Path) -> Result<()> {
    use std::io::{Read, Write};
    let Some(parent) = path
        .parent()
        .filter(|p| p.file_name().is_some_and(|n| n == ".quipu"))
    else {
        return Ok(());
    };
    if path.file_name().is_none_or(|n| n != "verifier.pk8") {
        return Ok(());
    }
    std::fs::create_dir_all(parent).map_err(|e| crate::Error::Store(e.to_string()))?;
    let mut options = std::fs::OpenOptions::new();
    options.read(true).append(true).create(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let mut file = options
        .open(parent.join(".gitignore"))
        .map_err(|e| crate::Error::Store(e.to_string()))?;
    let meta = file
        .metadata()
        .map_err(|e| crate::Error::Store(e.to_string()))?;
    if !meta.is_file() || meta.len() > 65536 {
        return Err(crate::Error::InvalidValue(
            "legacy key ignore file is not a bounded regular file".into(),
        ));
    }
    let mut text = String::new();
    file.read_to_string(&mut text)
        .map_err(|e| crate::Error::Store(e.to_string()))?;
    if text.lines().last() != Some("/verifier.pk8") {
        file.write_all(b"\n/verifier.pk8\n")
            .map_err(|e| crate::Error::Store(e.to_string()))?;
        file.sync_all()
            .map_err(|e| crate::Error::Store(e.to_string()))?;
    }
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
fn key_path(
    explicit: Option<&std::ffi::OsStr>,
    xdg: Option<&std::ffi::OsStr>,
    home: Option<&std::ffi::OsStr>,
) -> Result<std::path::PathBuf> {
    if let Some(path) = explicit {
        if path.is_empty() {
            return Err(crate::Error::InvalidValue(
                "QUIPU_SIGNING_KEY is empty".into(),
            ));
        }
        return Ok(path.into());
    }
    let base = xdg.filter(|s| !s.is_empty()).map(std::path::PathBuf::from)
        .or_else(|| home.filter(|s| !s.is_empty()).map(|s| Path::new(s).join(".local/share")))
        .filter(|p| p.is_absolute())
        .ok_or_else(|| crate::Error::InvalidValue("signing key needs an absolute XDG_DATA_HOME or HOME, or explicit QUIPU_SIGNING_KEY".into()))?;
    Ok(base.join("quipu/verifier.pk8"))
}

#[cfg(not(target_arch = "wasm32"))]
pub fn load_or_generate(path: &Path) -> Result<Ed25519KeyPair> {
    protect_repository_key(path)?;
    quipu_signing::load_or_generate(path).map_err(Into::into)
}

#[cfg(test)]
#[path = "signing_shared_tests.rs"]
mod shared_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use ring::rand::SystemRandom;

    #[test]
    fn ejzx_signing_defaults_are_absolute_user_data_paths() {
        use std::ffi::OsStr;
        assert_eq!(
            key_path(
                None,
                Some(OsStr::new("/data")),
                Some(OsStr::new("/home/user"))
            )
            .unwrap(),
            Path::new("/data/quipu/verifier.pk8")
        );
        assert_eq!(
            key_path(None, None, Some(OsStr::new("/home/user"))).unwrap(),
            Path::new("/home/user/.local/share/quipu/verifier.pk8")
        );
        assert_eq!(
            key_path(Some(OsStr::new("explicit.pk8")), None, None).unwrap(),
            Path::new("explicit.pk8")
        );
        assert!(key_path(None, None, None).is_err());
        assert!(key_path(None, Some(OsStr::new("relative")), None).is_err());
        assert!(key_path(None, None, Some(OsStr::new("relative"))).is_err());
        assert!(key_path(Some(OsStr::new("")), None, Some(OsStr::new("/home/user"))).is_err());
    }

    #[test]
    fn ejzx_legacy_key_creation_is_ignored_without_hiding_project_files() {
        let root = tempfile::tempdir().unwrap();
        let key = root.path().join(".quipu/verifier.pk8");
        std::fs::create_dir_all(key.parent().unwrap()).unwrap();
        let ignore = key.parent().unwrap().join(".gitignore");
        std::fs::write(&ignore, "!project\n!graph/**\n!/verifier.pk8\n").unwrap();
        load_or_generate(&key).unwrap();
        let before = std::fs::read(&ignore).unwrap();
        protect_repository_key(&key).unwrap();
        assert_eq!(std::fs::read(&ignore).unwrap(), before);
        assert!(
            String::from_utf8(before)
                .unwrap()
                .ends_with("/verifier.pk8\n")
        );
    }

    #[cfg(unix)]
    #[test]
    fn ejzx_legacy_ignore_symlink_refuses_before_key_creation() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join(".quipu");
        std::fs::create_dir(&dir).unwrap();
        let target = root.path().join("untouched");
        std::fs::write(&target, "original").unwrap();
        std::os::unix::fs::symlink(&target, dir.join(".gitignore")).unwrap();
        assert!(load_or_generate(&dir.join("verifier.pk8")).is_err());
        assert!(!dir.join("verifier.pk8").exists());
        assert_eq!(std::fs::read_to_string(target).unwrap(), "original");
    }

    #[test]
    fn sign_roundtrips_and_rejects_tampering() {
        let rng = SystemRandom::new();
        let doc = Ed25519KeyPair::generate_pkcs8(&rng).unwrap();
        let kp = Ed25519KeyPair::from_pkcs8(doc.as_ref()).unwrap();
        let pk = public_key_hex(&kp);
        let msg = verdict_message(
            "has-test",
            "sym1",
            "satisfied",
            "fnv1a:ab",
            "committed",
            "quipu",
        );
        let sig = sign_hex(&kp, &msg);
        assert!(
            verify_hex(&pk, &msg, &sig),
            "a genuine signature must verify"
        );
        // tampered message
        let tampered = verdict_message(
            "has-test",
            "sym1",
            "unsatisfied",
            "fnv1a:ab",
            "committed",
            "quipu",
        );
        assert!(
            !verify_hex(&pk, &tampered, &sig),
            "flipping the outcome must break the sig"
        );
        // wrong key
        let doc2 = Ed25519KeyPair::generate_pkcs8(&rng).unwrap();
        let kp2 = Ed25519KeyPair::from_pkcs8(doc2.as_ref()).unwrap();
        assert!(
            !verify_hex(&public_key_hex(&kp2), &msg, &sig),
            "a different key must not verify"
        );
        // garbage inputs never panic
        assert!(!verify_hex("zz", &msg, &sig));
        assert!(!verify_hex(&pk, &msg, "not-hex"));
    }

    #[test]
    fn load_or_generate_persists_and_reloads_same_key() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("keys").join("verifier.pk8");
        let kp1 = load_or_generate(&path).unwrap();
        assert!(path.exists(), "key file must be created");
        let pk1 = public_key_hex(&kp1);
        // second load reads the same persisted key
        let kp2 = load_or_generate(&path).unwrap();
        assert_eq!(
            pk1,
            public_key_hex(&kp2),
            "reload must yield the same identity"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "key file must be 0600");
        }
    }
}
