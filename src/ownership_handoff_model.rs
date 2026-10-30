//! Experimental, I/O-free model of the ownership-handoff design's P0 contract.
//!
//! This is NOT a verifier, journal implementation, enrollment path or wire
//! profile. Oracle observations below are assumptions for model exploration,
//! never credentials. Store constructors, commit hooks and replication do not
//! call this module. Physical durability, signatures and native placement still
//! require separately reviewed adapters and acceptance.

mod records;
mod state;
mod transitions;

pub use records::*;
pub use state::*;
pub use transitions::*;

/// A model refusal. No runtime authority is created by this result type.
pub type ModelResult<T> = std::result::Result<T, String>;

pub(super) fn require(condition: bool, why: &str) -> ModelResult<()> {
    if condition { Ok(()) } else { Err(why.into()) }
}

#[cfg(test)]
mod tests;
