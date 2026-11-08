use super::*;
use std::collections::{BTreeSet, HashSet};

pub(super) struct Walk {
    pub(super) head: WitnessHead,
    pub(super) next_attempt: u64,
}

impl Memory {
    fn read(
        &self,
        key: Key,
        seen: &mut BTreeSet<Key>,
        reads: &mut usize,
    ) -> Result<Option<Entry>, ()> {
        *reads += 1;
        if *reads > self.read_budget || *reads > MAX_READS {
            return Err(());
        }
        seen.insert(key);
        self.entries.get(&key).map(|b| Entry::decode(b)).transpose()
    }

    /// Anchor is selected by the modeled caller's journal role. Never infer it
    /// from a copied database. This adapter performs no authenticated I/O.
    pub(super) fn walk(&self, anchor: u64) -> Result<Walk, ()> {
        if anchor > MAX_GENERATION || self.entries.len() > MAX_RECORDS {
            return Err(());
        }
        let mut seen = BTreeSet::new();
        let mut reads = 0;
        let mut operations = HashSet::new();
        let mut generation = anchor;
        let Some(Entry::Active(mut authority)) =
            self.read(Key::Active(generation), &mut seen, &mut reads)?
        else {
            return Err(());
        };
        if authority.store_id != self.store_id || authority.generation != generation {
            return Err(());
        }
        let mut attempt = 0;
        let head = loop {
            if generation > MAX_GENERATION || attempt > MAX_ATTEMPT {
                return Err(());
            }
            let reserved = self.read(Key::Reserved(generation, attempt), &mut seen, &mut reads)?;
            let outcome = self.read(Key::Outcome(generation, attempt), &mut seen, &mut reads)?;
            match (reserved, outcome) {
                (None, None) => {
                    if self
                        .read(
                            Key::Reserved(generation, attempt + 1),
                            &mut seen,
                            &mut reads,
                        )?
                        .is_some()
                        || self
                            .read(Key::Active(generation + 1), &mut seen, &mut reads)?
                            .is_some()
                    {
                        return Err(());
                    }
                    break WitnessHead::Active { authority };
                }
                (Some(Entry::Reserved(request)), outcome) => {
                    if request.source != authority || !operations.insert(request.operation.clone())
                    {
                        return Err(());
                    }
                    match outcome {
                        Some(Entry::Abandoned(a)) if a.request == request => {
                            attempt += 1;
                        }
                        None => {
                            if self
                                .read(
                                    Key::Reserved(generation, attempt + 1),
                                    &mut seen,
                                    &mut reads,
                                )?
                                .is_some()
                                || self
                                    .read(Key::Active(generation + 1), &mut seen, &mut reads)?
                                    .is_some()
                            {
                                return Err(());
                            }
                            break WitnessHead::Reserved { request };
                        }
                        Some(Entry::Retired(certificate)) if certificate.request == request => {
                            if self
                                .read(
                                    Key::Reserved(generation, attempt + 1),
                                    &mut seen,
                                    &mut reads,
                                )?
                                .is_some()
                            {
                                return Err(());
                            }
                            match self.read(Key::Active(generation + 1), &mut seen, &mut reads)? {
                                None => break WitnessHead::Retired { certificate },
                                Some(Entry::Active(next))
                                    if certificate.successor(&next.epoch).ok().as_ref()
                                        == Some(&next) =>
                                {
                                    authority = next;
                                    generation += 1;
                                    attempt = 0;
                                }
                                _ => return Err(()),
                            }
                        }
                        _ => return Err(()),
                    }
                }
                _ => return Err(()),
            }
        };
        // Full bounded map inspection makes hidden/skipped keys UNKNOWN too,
        // beyond the explicitly fetched one-past-head controls above.
        if self
            .entries
            .keys()
            .any(|key| key.generation() >= anchor && !seen.contains(key))
        {
            return Err(());
        }
        Ok(Walk {
            head,
            next_attempt: attempt,
        })
    }
}
