//! Exclusive private claim, retained even when a capture attempt fails.
#[cfg(unix)]
pub(super) fn claim(db_path: &str, nonce: &str) -> std::io::Result<std::fs::File> {
    use rustix::fs::{Mode, OFlags};
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
    let invalid = || std::io::Error::other("private claim refused");
    let db = std::fs::canonicalize(db_path)?;
    let directory = db.parent().ok_or_else(invalid)?.join("query-captures");
    match std::fs::DirBuilder::new().mode(0o700).create(&directory) {
        Ok(()) => (),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => (),
        Err(error) => return Err(error),
    }
    let dir = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(&directory)?;
    let metadata = dir.metadata()?;
    if metadata.uid() != rustix::process::geteuid().as_raw() || metadata.mode() & 0o077 != 0 {
        return Err(invalid());
    }
    let file = rustix::fs::openat(
        &dir,
        format!("{nonce}.json"),
        OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::RUSR | Mode::WUSR,
    )?;
    Ok(file.into())
}

#[cfg(not(unix))]
pub(super) fn claim(_: &str, _: &str) -> std::io::Result<std::fs::File> {
    Err(std::io::Error::other("private capture unsupported"))
}
