use super::*;
use serde::{Deserialize, Serialize};

pub(super) const MAX_RECORDS: usize = 512;
pub(super) const MAX_GENERATION: u64 = 32;
pub(super) const MAX_ATTEMPT: u64 = 32;
pub(super) const MAX_READS: usize = 256;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub(super) enum Key {
    Active(u64),
    Reserved(u64, u64),
    Outcome(u64, u64),
}
impl Key {
    pub(super) fn generation(&self) -> u64 {
        match *self {
            Self::Active(g) | Self::Reserved(g, _) | Self::Outcome(g, _) => g,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "body", deny_unknown_fields)]
pub(super) enum Entry {
    Active(Authority),
    Reserved(Request),
    Retired(Certificate),
    Abandoned(Abandonment),
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    version: u8,
    entry: Entry,
}

impl Entry {
    pub(super) fn validate(&self) -> bool {
        match self {
            Self::Active(a) => a.validate(),
            Self::Reserved(r) => r.validate(),
            Self::Retired(c) => c.validate(),
            Self::Abandoned(a) => a.validate(),
        }
        .is_ok()
    }
    pub(super) fn bytes(&self) -> Result<Vec<u8>, ()> {
        if !self.validate() {
            return Err(());
        }
        let bytes = serde_json::to_vec(&Envelope {
            version: 1,
            entry: self.clone(),
        })
        .map_err(|_| ())?;
        if bytes.len() > super::super::MAX_RECORD_BYTES {
            return Err(());
        }
        Ok(bytes)
    }
    pub(super) fn decode(bytes: &[u8]) -> Result<Self, ()> {
        if bytes.len() > super::super::MAX_RECORD_BYTES {
            return Err(());
        }
        let body: Envelope = serde_json::from_slice(bytes).map_err(|_| ())?;
        if body.version != 1 || body.entry.bytes()? != bytes {
            return Err(());
        }
        Ok(body.entry)
    }
}
