use super::*;
use std::collections::{BTreeSet, HashSet};

pub(super) struct Walk {
    pub(super) head: WitnessHead,
    pub(super) next_attempt: u64,
    /// (canonical key, backend version) for every present record read.
    pub(super) versions: Vec<(String, String)>,
}

impl<B: Objects> ObjectWitness<B> {
    fn read(
        &self,
        key: Key,
        seen: &mut BTreeSet<Key>,
        reads: &mut usize,
        versions: &mut Vec<(String, String)>,
    ) -> Result<Option<Entry>, ()> {
        *reads += 1;
        if *reads > self.backend.read_budget() || *reads > MAX_READS {
            return Err(());
        }
        seen.insert(key);
        match self.backend.get(&objects::name(key)) {
            Reply::Ok(Some(o)) => {
                let entry = Entry::decode(&o.bytes)?;
                versions.push((objects::name(key), o.version));
                Ok(Some(entry))
            }
            Reply::Ok(None) => Ok(None),
            _ => Err(()),
        }
    }

    /// Anchor is selected by the modeled caller's journal role. Never infer it
    /// from a copied database. This adapter performs no authenticated I/O.
    pub(super) fn walk(&self, anchor: u64) -> Result<Walk, ()> {
        if anchor > MAX_GENERATION {
            return Err(());
        }
        let mut seen = BTreeSet::new();
        let mut reads = 0;
        let mut versions = Vec::new();
        let mut operations = HashSet::new();
        let mut generation = anchor;
        let Some(Entry::Active(mut authority)) = self.read(
            Key::Active(generation),
            &mut seen,
            &mut reads,
            &mut versions,
        )?
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
            let reserved = self.read(
                Key::Reserved(generation, attempt),
                &mut seen,
                &mut reads,
                &mut versions,
            )?;
            let outcome = self.read(
                Key::Outcome(generation, attempt),
                &mut seen,
                &mut reads,
                &mut versions,
            )?;
            match (reserved, outcome) {
                (None, None) => {
                    if self
                        .read(
                            Key::Reserved(generation, attempt + 1),
                            &mut seen,
                            &mut reads,
                            &mut versions,
                        )?
                        .is_some()
                        || self
                            .read(
                                Key::Active(generation + 1),
                                &mut seen,
                                &mut reads,
                                &mut versions,
                            )?
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
                                    &mut versions,
                                )?
                                .is_some()
                                || self
                                    .read(
                                        Key::Active(generation + 1),
                                        &mut seen,
                                        &mut reads,
                                        &mut versions,
                                    )?
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
                                    &mut versions,
                                )?
                                .is_some()
                            {
                                return Err(());
                            }
                            match self.read(
                                Key::Active(generation + 1),
                                &mut seen,
                                &mut reads,
                                &mut versions,
                            )? {
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
        // Listed AFTER the reads, so a key created during the walk is seen.
        // An unparseable name under the lineage is UNKNOWN, never ignored.
        let Reply::Ok(names) = self.backend.list(MAX_RECORDS) else {
            return Err(());
        };
        for name in names {
            let key = objects::parse(&name).ok_or(())?;
            if key.generation() >= anchor && !seen.contains(&key) {
                return Err(());
            }
        }
        Ok(Walk {
            head,
            next_attempt: attempt,
            versions,
        })
    }
}
