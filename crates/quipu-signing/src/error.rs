//! Errors from host-file key loading and generation.
use std::fmt;

/// A key custody or encoding error; no key bytes are included.
#[derive(Debug)]
pub enum Error {
    /// Host-file I/O or invalid PKCS#8 data.
    Store(String),
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Store(message) => formatter.write_str(message),
        }
    }
}
impl std::error::Error for Error {}
/// Result of a key custody operation.
pub type Result<T> = std::result::Result<T, Error>;
