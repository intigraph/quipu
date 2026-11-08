//! Dormant E1 enrollment inventory. No production constructor calls this module.
//! A matching binding means only that journal/witness admission must follow.
//! This reader does not establish protected-file ownership, permissions, witness
//! freshness or transaction authority. Those remain prerequisites for wiring.
#![cfg_attr(not(test), allow(dead_code))]

use std::fs::File;
use std::io::Read;
use std::path::{Component, Path};

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use crate::ownership_handoff_model::AuthorityUnit;

const VERSION: u32 = 1;
const MAX_BYTES: usize = 64 * 1024;
const MAX_RECORDS: usize = 64;
const TRAILER: &str = "\nsha256:";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Database {
    pub path: String,
    pub device: u64,
    pub inode: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Entry {
    pub store_id: String,
    pub unit: AuthorityUnit,
    pub database: Database,
    pub journal: String,
    pub witness: String,
    pub enrolled_at_generation: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Inventory {
    pub version: u32,
    pub records: Vec<Entry>,
}

/// Reserved marker body only; no SQL installation or startup reader is added.
/// Object presence with an invalid body must be passed as `Marker::Unknown`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Binding {
    pub version: u32,
    pub store_id: String,
    pub inventory_digest: String,
}

#[derive(Clone, Copy)]
pub(super) enum Marker<'a> {
    Absent,
    Known(&'a Binding),
    Unknown,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Decision {
    Legacy,
    NeedsJournalAndWitness,
    Refuse,
}

fn text(value: &str) -> bool {
    !value.is_empty() && value.len() <= 1024 && !value.chars().any(char::is_control)
}

// Lexical validation only. The caller must obtain the actual canonical path and
// device/inode from an open database, never from these serialized claims.
fn absolute(value: &str) -> bool {
    text(value)
        && Path::new(value).is_absolute()
        && !value.contains("//")
        && !value.ends_with('/')
        && !value.split('/').any(|part| part == "." || part == "..")
        && Path::new(value)
            .components()
            .all(|c| matches!(c, Component::RootDir | Component::Normal(_)))
}

impl Entry {
    fn valid(&self) -> bool {
        text(&self.store_id)
            && absolute(&self.database.path)
            && self.database.inode != 0
            && absolute(&self.journal)
            && self.journal != self.database.path
            && !self.witness.is_empty()
            && self.witness.len() <= 128
            && self
                .witness
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"_-.".contains(&c))
    }

    pub(super) fn digest(&self) -> Result<String, String> {
        if !self.valid() {
            return Err("invalid entry".into());
        }
        let body = serde_json::to_vec(self).map_err(|e| e.to_string())?;
        Ok(hex::encode(Sha256::digest(body)))
    }
}

impl Inventory {
    fn validate(&self) -> Result<(), String> {
        if self.version != VERSION || self.records.len() > MAX_RECORDS {
            return Err("inventory version or count".into());
        }
        for (i, entry) in self.records.iter().enumerate() {
            if !entry.valid() {
                return Err("invalid inventory entry".into());
            }
            if self.records[..i].iter().any(|old| {
                old.store_id == entry.store_id
                    || old.database.path == entry.database.path
                    || (old.database.device, old.database.inode)
                        == (entry.database.device, entry.database.inode)
                    || old.journal == entry.journal
            }) {
                return Err("ambiguous inventory binding".into());
            }
        }
        Ok(())
    }

    pub(super) fn encode(&self) -> Result<Vec<u8>, String> {
        self.validate()?;
        let body = serde_json::to_vec(self).map_err(|e| e.to_string())?;
        let mut bytes = body.clone();
        bytes.extend_from_slice(TRAILER.as_bytes());
        bytes.extend_from_slice(hex::encode(Sha256::digest(body)).as_bytes());
        bytes.push(b'\n');
        if bytes.len() > MAX_BYTES {
            return Err("oversized inventory".into());
        }
        Ok(bytes)
    }

    pub(super) fn decode(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > MAX_BYTES {
            return Err("oversized inventory".into());
        }
        let text = std::str::from_utf8(bytes).map_err(|e| e.to_string())?;
        let (body, checksum) = text.rsplit_once(TRAILER).ok_or("missing checksum")?;
        if checksum.strip_suffix('\n')
            != Some(hex::encode(Sha256::digest(body.as_bytes())).as_str())
        {
            return Err("inventory checksum mismatch".into());
        }
        let inventory: Self = serde_json::from_str(body).map_err(|e| e.to_string())?;
        // Strict canonical bytes also reject alternate encodings/orderings.
        if inventory.encode()? != bytes {
            return Err("noncanonical inventory".into());
        }
        Ok(inventory)
    }
}

impl Binding {
    pub(super) fn decode(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > 2048 {
            return Err("oversized marker".into());
        }
        let binding: Self = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
        if !binding.valid() {
            return Err("invalid marker".into());
        }
        Ok(binding)
    }

    fn valid(&self) -> bool {
        self.version == VERSION
            && text(&self.store_id)
            && self.inventory_digest.len() == 64
            && self
                .inventory_digest
                .bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
    }
}

/// Explicit configured path only: no fallback, default, search or write.
pub(super) fn read(path: &Path) -> Result<Inventory, String> {
    if !path.is_absolute() {
        return Err("inventory path is not absolute".into());
    }
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|e| e.to_string())?
        .take((MAX_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    Inventory::decode(&bytes)
}

/// `None` means explicitly unconfigured, not an unreadable/corrupt inventory.
/// Callers must refuse on `read` errors rather than converting them to `None`.
pub(super) fn decide(
    marker: Marker<'_>,
    inventory: Option<&Inventory>,
    actual: &Database,
) -> Decision {
    if !absolute(&actual.path) || actual.inode == 0 {
        return Decision::Refuse;
    }
    let Some(inventory) = inventory else {
        return if matches!(marker, Marker::Absent) {
            Decision::Legacy
        } else {
            Decision::Refuse
        };
    };
    if inventory.validate().is_err() {
        return Decision::Refuse;
    }
    let candidates: Vec<_> = inventory
        .records
        .iter()
        .filter(|entry| {
            entry.database.path == actual.path
                || (entry.database.device, entry.database.inode) == (actual.device, actual.inode)
        })
        .collect();
    match marker {
        Marker::Absent if candidates.is_empty() => Decision::Legacy,
        Marker::Known(binding) if binding.valid() && candidates.len() == 1 => {
            let entry = candidates[0];
            if entry.database == *actual
                && entry.store_id == binding.store_id
                && entry.digest().ok().as_ref() == Some(&binding.inventory_digest)
            {
                Decision::NeedsJournalAndWitness
            } else {
                Decision::Refuse
            }
        }
        _ => Decision::Refuse,
    }
}

#[cfg(test)]
#[path = "ownership_inventory_tests.rs"]
mod tests;
