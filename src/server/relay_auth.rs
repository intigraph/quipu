//! Private per-relay transport credentials; these are not read grants.
use std::path::Path;

#[cfg(unix)]
const MAX_BYTES: u64 = 4096;

pub(super) fn validate_url(value: &str) -> Result<(), &'static str> {
    let url = url::Url::parse(value).map_err(|_| "invalid relay upstream URL")?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("relay upstream URL must be HTTP(S) without user info, query or fragment");
    }
    Ok(())
}

/// Read anew for each operation: file removal or rotation cannot fall back to
/// a credential cached by an earlier stream. Errors contain no path or bytes.
pub(super) fn credential_header(file: Option<&Path>) -> Result<Option<String>, &'static str> {
    let Some(path) = file else { return Ok(None) };
    if !path.is_absolute() {
        return Err("relay upstream credential file must be an absolute path");
    }
    read_private(path).map(Some)
}

#[cfg(unix)]
fn read_private(path: &Path) -> Result<String, &'static str> {
    use std::io::Read;
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
    let file = std::fs::OpenOptions::new()
        .read(true)
        // Do not follow a final symlink, or block while opening a FIFO/device.
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .map_err(|_| "cannot open relay upstream credential file")?;
    let metadata = file
        .metadata()
        .map_err(|_| "cannot inspect relay upstream credential file")?;
    let owner = rustix::process::geteuid().as_raw();
    if !metadata.is_file() || metadata.uid() != owner || metadata.permissions().mode() & 0o077 != 0
    {
        return Err(
            "relay upstream credential file must be a private regular file owned by this user",
        );
    }
    if metadata.len() > MAX_BYTES {
        return Err("relay upstream credential file exceeds 4096 bytes");
    }
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "cannot read relay upstream credential file")?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err("relay upstream credential file exceeds 4096 bytes");
    }
    let token = bytes
        .strip_suffix(b"\r\n")
        .or_else(|| bytes.strip_suffix(b"\n"))
        .unwrap_or(&bytes);
    if token.is_empty()
        || !token
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || b"-._~+/=".contains(b))
    {
        return Err("relay upstream credential file must contain one nonempty bearer token");
    }
    // The accepted alphabet above is ASCII, with no whitespace/header delimiters.
    Ok(format!(
        "Bearer {}",
        std::str::from_utf8(token).map_err(|_| "invalid relay upstream credential")?
    ))
}

#[cfg(not(unix))]
fn read_private(_: &Path) -> Result<String, &'static str> {
    Err("private relay upstream credential files are unsupported on this platform")
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::{PermissionsExt, symlink};

    #[test]
    fn private_file_validation_and_rotation() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("private-fixture");
        let write = |bytes: &[u8], mode| {
            if path.exists() {
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
            }
            std::fs::write(&path, bytes).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
        };
        assert_eq!(credential_header(None), Ok(None));
        assert!(credential_header(Some(Path::new("relative"))).is_err());
        assert!(credential_header(Some(&path)).is_err());
        for (bytes, mode) in [
            (b"first-fixture".as_slice(), 0o600),
            (b"second-fixture\n".as_slice(), 0o400),
            (b"third-fixture\r\n".as_slice(), 0o600),
        ] {
            write(bytes, mode);
            let expected = format!("Bearer {}", std::str::from_utf8(bytes).unwrap().trim_end());
            assert_eq!(credential_header(Some(&path)), Ok(Some(expected)));
        }
        let oversized = vec![b'x'; 4097];
        for bytes in [
            b"".as_slice(),
            b"one\ntwo",
            b"one\r\nInjected: value",
            b"\xff",
            oversized.as_slice(),
        ] {
            write(bytes, 0o600);
            let error = credential_header(Some(&path)).unwrap_err();
            assert!(!error.contains("private-fixture"));
            assert!(!error.contains("Injected"));
        }
        for mode in [0o644, 0o640, 0o606] {
            write(b"private-fixture", mode);
            assert!(credential_header(Some(&path)).is_err());
        }
        std::fs::remove_file(&path).unwrap();
        assert!(credential_header(Some(dir.path())).is_err());
        symlink(dir.path().join("absent"), &path).unwrap();
        assert!(credential_header(Some(&path)).is_err());
    }

    #[test]
    fn upstream_url_rejects_embedded_secrets_and_non_http_transports() {
        for value in ["https://fixture.invalid", "http://127.0.0.1:1234/base"] {
            assert_eq!(validate_url(value), Ok(()));
        }
        for value in [
            "https://fixture-secret@fixture.invalid",
            "https://fixture.invalid/?fixture-secret",
            "https://fixture.invalid/#fixture-secret",
            "file:///fixture-secret",
            "invalid-fixture-secret",
        ] {
            let error = validate_url(value).unwrap_err();
            assert!(!error.contains("fixture-secret"));
        }
    }
}
