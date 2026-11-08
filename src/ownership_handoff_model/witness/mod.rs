//! W1: dormant, in-memory candidate witness. No Store, startup, signer,
//! transport, clock, journal writer or runtime authority is connected here.
//! Model records and caller-supplied journal observations are not credentials.
#![allow(dead_code)]
use super::{Abandonment, Authority, Certificate, Journal, Request, WitnessHead};
use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex};
mod records;
mod walk;
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

#[derive(Default)]
struct Memory {
    store_id: String,
    entries: BTreeMap<Key, Vec<u8>>,
    faults: VecDeque<Fault>,
    read_budget: usize,
}

#[derive(Clone)]
pub(crate) struct InMemoryWitness(Arc<Mutex<Memory>>);

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
        Reply::Ok(Self(Arc::new(Mutex::new(Memory {
            store_id: genesis.store_id,
            entries: BTreeMap::from([(Key::Active(0), bytes)]),
            read_budget: MAX_READS,
            ..Memory::default()
        }))))
    }
    pub(crate) fn inject(&self, fault: Fault) -> Reply<()> {
        self.call(false, |memory| {
            if memory.faults.len() >= MAX_RECORDS {
                return Reply::Unknown;
            }
            memory.faults.push_back(fault);
            Reply::Ok(())
        })
    }
    fn call<T>(&self, consume: bool, f: impl FnOnce(&mut Memory) -> Reply<T>) -> Reply<T> {
        // Nonblocking admission: lock contention/poisoning is UNKNOWN.
        let Ok(mut memory) = self.0.try_lock() else {
            return Reply::Unknown;
        };
        let fault = if consume {
            memory.faults.pop_front()
        } else {
            None
        };
        match fault {
            Some(Fault::Unknown | Fault::Slow) => Reply::Unknown,
            Some(Fault::Conflict) => Reply::Conflict,
            _ => match f(&mut memory) {
                Reply::Ok(_) if matches!(fault, Some(Fault::LostReply)) => Reply::Unknown,
                other => other,
            },
        }
    }
}

impl Memory {
    fn create(
        &mut self,
        key: Key,
        entry: &Entry,
        check: impl FnOnce(&Self) -> Reply<()>,
    ) -> Reply<()> {
        let Ok(bytes) = entry.bytes() else {
            return Reply::Conflict;
        };
        if let Some(old) = self.entries.get(&key) {
            return match Entry::decode(old) {
                Ok(_) if old == &bytes => Reply::Ok(()),
                Ok(_) => Reply::Conflict,
                Err(()) => Reply::Unknown,
            };
        }
        if self.entries.len() >= MAX_RECORDS {
            return Reply::Unknown;
        }
        match check(self) {
            Reply::Ok(()) => {
                self.entries.insert(key, bytes);
                Reply::Ok(())
            }
            other => other,
        }
    }
}

impl AuthorityWitness for InMemoryWitness {
    fn head(&self, store_id: &str, anchor: u64) -> Reply<WitnessHead> {
        self.call(true, |memory| {
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
        self.call(true, |memory| {
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
                    for (key, bytes) in &m.entries {
                        if matches!(key, Key::Reserved(..)) {
                            let Ok(Entry::Reserved(old)) = Entry::decode(bytes) else {
                                return Reply::Unknown;
                            };
                            if old.operation == request.operation {
                                return Reply::Conflict;
                            }
                        }
                    }
                    Reply::Ok(())
                },
            )
        })
    }
    fn retire(&self, attempt: u64, certificate: &Certificate) -> Reply<()> {
        self.call(true, |memory| {
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
        self.call(true, |memory| {
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
        self.call(true, |memory| {
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
