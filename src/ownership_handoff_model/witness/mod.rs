//! W1: dormant, in-memory candidate witness. No Store, startup, signer,
//! transport, clock, journal writer or runtime authority is connected here.
//! Model records and caller-supplied journal observations are not credentials.
#![allow(dead_code)]
use super::{Abandonment, Authority, Certificate, Journal, Request, WitnessHead};
use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex};
#[cfg(test)]
mod conformance;
pub(crate) mod objects;
mod records;
#[cfg(all(feature = "ownership-witness-s3", not(target_arch = "wasm32")))]
pub(crate) mod s3;
mod walk;
use objects::{Created, Object, Objects};
use records::*;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Reply<T> {
    Ok(T),
    Conflict,
    Unknown,
}

/// Deterministic outcomes, not real sleeps or network failures. `LostReply`
/// commits a successful create before returning UNKNOWN; readback stays exact.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Fault {
    Unknown,
    Slow,
    Conflict,
    LostReply,
}

pub(crate) trait AuthorityWitness {
    fn head(&self, store_id: &str, anchor: u64) -> Reply<WitnessHead>;
    fn reserve(&self, attempt: u64, request: &Request) -> Reply<()>;
    fn retire(&self, attempt: u64, certificate: &Certificate) -> Reply<()>;
    fn abandon(
        &self,
        attempt: u64,
        outcome: &Abandonment,
        source: &Journal,
        sealed: bool,
    ) -> Reply<()>;
    fn activate(&self, authority: &Authority) -> Reply<()>;
}

/// Private in-memory backend state. Keys are typed because the in-memory
/// store can only ever hold canonical witness keys.
#[derive(Default)]
struct Memory {
    entries: BTreeMap<Key, Vec<u8>>,
    faults: VecDeque<Fault>,
    read_budget: usize,
}

/// The W1 deterministic backend. Lock contention or poisoning is UNKNOWN.
#[derive(Clone, Default)]
pub(crate) struct MemoryObjects(Arc<Mutex<Memory>>);

impl Objects for MemoryObjects {
    fn get(&self, key: &str) -> Reply<Option<Object>> {
        let Ok(memory) = self.0.try_lock() else {
            return Reply::Unknown;
        };
        let Some(key) = objects::parse(key) else {
            return Reply::Unknown;
        };
        Reply::Ok(memory.entries.get(&key).map(|bytes| Object {
            version: version(bytes),
            bytes: bytes.clone(),
        }))
    }
    fn create(&self, key: &str, bytes: &[u8]) -> Reply<Created> {
        let Ok(mut memory) = self.0.try_lock() else {
            return Reply::Unknown;
        };
        let Some(key) = objects::parse(key) else {
            return Reply::Unknown;
        };
        if memory.entries.contains_key(&key) {
            return Reply::Ok(Created::Exists);
        }
        memory.entries.insert(key, bytes.to_vec());
        Reply::Ok(Created::New)
    }
    fn list(&self, limit: usize) -> Reply<Vec<String>> {
        let Ok(memory) = self.0.try_lock() else {
            return Reply::Unknown;
        };
        if memory.entries.len() > limit {
            return Reply::Unknown;
        }
        Reply::Ok(memory.entries.keys().map(|k| objects::name(*k)).collect())
    }
    fn fault(&self) -> Option<Fault> {
        self.0
            .try_lock()
            .ok()
            .and_then(|mut m| m.faults.pop_front())
    }
    fn read_budget(&self) -> usize {
        self.0.try_lock().map_or(0, |m| m.read_budget)
    }
}

fn version(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(bytes))
}

/// The witness logic over any create-only backend. Atomicity comes from the
/// deterministic create-only key, never from a backend-wide lock.
#[derive(Clone)]
pub(crate) struct ObjectWitness<B> {
    store_id: String,
    backend: B,
}

pub(crate) type InMemoryWitness = ObjectWitness<MemoryObjects>;

impl InMemoryWitness {
    /// Seeds an assumed enrolled genesis in a private model only. This is NOT
    /// enrollment and must never be exposed as a production constructor.
    pub(crate) fn new(genesis: Authority) -> Reply<Self> {
        if genesis.generation != 0 {
            return Reply::Conflict;
        }
        let Ok(bytes) = Entry::Active(genesis.clone()).bytes() else {
            return Reply::Conflict;
        };
        Reply::Ok(Self {
            store_id: genesis.store_id,
            backend: MemoryObjects(Arc::new(Mutex::new(Memory {
                entries: BTreeMap::from([(Key::Active(0), bytes)]),
                read_budget: MAX_READS,
                ..Memory::default()
            }))),
        })
    }
    pub(crate) fn inject(&self, fault: Fault) -> Reply<()> {
        let Ok(mut memory) = self.backend.0.try_lock() else {
            return Reply::Unknown;
        };
        if memory.faults.len() >= MAX_RECORDS {
            return Reply::Unknown;
        }
        memory.faults.push_back(fault);
        Reply::Ok(())
    }
}

impl<B: Objects> ObjectWitness<B> {
    /// Attach the witness logic to an existing lineage in `backend`. Nothing
    /// is written; an unreadable or foreign lineage reads as UNKNOWN later.
    pub(crate) fn attach(store_id: String, backend: B) -> Self {
        Self { store_id, backend }
    }
    fn call<T>(&self, f: impl FnOnce(&Self) -> Reply<T>) -> Reply<T> {
        let fault = self.backend.fault();
        match fault {
            Some(Fault::Unknown | Fault::Slow) => Reply::Unknown,
            Some(Fault::Conflict) => Reply::Conflict,
            _ => match f(self) {
                Reply::Ok(_) if matches!(fault, Some(Fault::LostReply)) => Reply::Unknown,
                other => other,
            },
        }
    }
    /// Same body: already done. Different valid body: conflict. Anything
    /// unreadable: UNKNOWN.
    fn readback(&self, key: Key, bytes: &[u8]) -> Reply<()> {
        match self.backend.get(&objects::name(key)) {
            Reply::Ok(Some(old)) => match Entry::decode(&old.bytes) {
                Ok(_) if old.bytes == bytes => Reply::Ok(()),
                Ok(_) => Reply::Conflict,
                Err(()) => Reply::Unknown,
            },
            _ => Reply::Unknown,
        }
    }
    fn create(&self, key: Key, entry: &Entry, check: impl FnOnce(&Self) -> Reply<()>) -> Reply<()> {
        let Ok(bytes) = entry.bytes() else {
            return Reply::Conflict;
        };
        match self.backend.get(&objects::name(key)) {
            Reply::Ok(Some(_)) => return self.readback(key, &bytes),
            Reply::Ok(None) => {}
            _ => return Reply::Unknown,
        }
        match self.backend.list(MAX_RECORDS) {
            Reply::Ok(keys) if keys.len() < MAX_RECORDS => {}
            _ => return Reply::Unknown,
        }
        match check(self) {
            Reply::Ok(()) => {}
            other => return other,
        }
        match self.backend.create(&objects::name(key), &bytes) {
            Reply::Ok(Created::New) => Reply::Ok(()),
            // Another writer won this exact key between check and create.
            Reply::Ok(Created::Exists) => self.readback(key, &bytes),
            _ => Reply::Unknown,
        }
    }
    /// Every reserved request in the lineage, for operation uniqueness.
    fn reserved_requests(&self) -> Result<Vec<Request>, ()> {
        let Reply::Ok(keys) = self.backend.list(MAX_RECORDS) else {
            return Err(());
        };
        let mut out = Vec::new();
        for name in keys {
            if !matches!(objects::parse(&name), Some(Key::Reserved(..))) {
                continue;
            }
            match self.backend.get(&name) {
                Reply::Ok(Some(old)) => match Entry::decode(&old.bytes) {
                    Ok(Entry::Reserved(request)) => out.push(request),
                    _ => return Err(()),
                },
                _ => return Err(()),
            }
        }
        Ok(out)
    }
}

impl<B: Objects> AuthorityWitness for ObjectWitness<B> {
    fn head(&self, store_id: &str, anchor: u64) -> Reply<WitnessHead> {
        self.call(|memory| {
            if store_id != memory.store_id {
                return Reply::Conflict;
            }
            match memory.walk(anchor) {
                Ok(w) => Reply::Ok(w.head),
                Err(()) => Reply::Unknown,
            }
        })
    }
    fn reserve(&self, attempt: u64, request: &Request) -> Reply<()> {
        self.call(|memory| {
            if attempt > MAX_ATTEMPT || request.source.generation >= MAX_GENERATION {
                return Reply::Conflict;
            }
            memory.create(
                Key::Reserved(request.source.generation, attempt),
                &Entry::Reserved(request.clone()),
                |m| {
                    let Ok(w) = m.walk(0) else {
                        return Reply::Unknown;
                    };
                    if w.head
                        != (WitnessHead::Active {
                            authority: request.source.clone(),
                        })
                        || w.next_attempt != attempt
                    {
                        return Reply::Conflict;
                    }
                    let Ok(reserved) = m.reserved_requests() else {
                        return Reply::Unknown;
                    };
                    if reserved
                        .iter()
                        .any(|old| old.operation == request.operation)
                    {
                        return Reply::Conflict;
                    }
                    Reply::Ok(())
                },
            )
        })
    }
    fn retire(&self, attempt: u64, certificate: &Certificate) -> Reply<()> {
        self.call(|memory| {
            if attempt > MAX_ATTEMPT {
                return Reply::Conflict;
            }
            memory.create(
                Key::Outcome(certificate.request.source.generation, attempt),
                &Entry::Retired(certificate.clone()),
                |m| {
                    let Ok(w) = m.walk(0) else {
                        return Reply::Unknown;
                    };
                    if w.head
                        == (WitnessHead::Reserved {
                            request: certificate.request.clone(),
                        })
                        && w.next_attempt == attempt
                    {
                        Reply::Ok(())
                    } else {
                        Reply::Conflict
                    }
                },
            )
        })
    }
    fn abandon(
        &self,
        attempt: u64,
        outcome: &Abandonment,
        source: &Journal,
        sealed: bool,
    ) -> Reply<()> {
        self.call(|memory| {
            if attempt > MAX_ATTEMPT {
                return Reply::Conflict;
            }
            // These are explicit model inputs, NOT real local-journal proof.
            // Even restored ACTIVE bytes do not erase a retained sealed cut.
            memory.create(
                Key::Outcome(outcome.request.source.generation, attempt),
                &Entry::Abandoned(outcome.clone()),
                |m| {
                    if sealed
                        || source
                            != &(Journal::Active {
                                authority: outcome.request.source.clone(),
                            })
                    {
                        return Reply::Conflict;
                    }
                    let Ok(w) = m.walk(0) else {
                        return Reply::Unknown;
                    };
                    if w.head
                        == (WitnessHead::Reserved {
                            request: outcome.request.clone(),
                        })
                        && w.next_attempt == attempt
                    {
                        Reply::Ok(())
                    } else {
                        Reply::Conflict
                    }
                },
            )
        })
    }
    fn activate(&self, authority: &Authority) -> Reply<()> {
        self.call(|memory| {
            if authority.generation == 0 || authority.generation > MAX_GENERATION {
                return Reply::Conflict;
            }
            memory.create(
                Key::Active(authority.generation),
                &Entry::Active(authority.clone()),
                |m| {
                    let Ok(w) = m.walk(0) else {
                        return Reply::Unknown;
                    };
                    match w.head {
                        WitnessHead::Retired { certificate }
                            if certificate.successor(&authority.epoch).ok().as_ref()
                                == Some(authority) =>
                        {
                            Reply::Ok(())
                        }
                        _ => Reply::Conflict,
                    }
                },
            )
        })
    }
}

#[cfg(test)]
mod tests;
