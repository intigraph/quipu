use super::*;

/// Model actions expose separate durable boundaries for crash enumeration.
/// No variant calls Store, filesystem, signing, IPC or a network service.
#[derive(Clone, Debug)]
#[allow(missing_docs)]
pub enum Action {
    /// Reserve one exact destination and successor without fencing source writes.
    Prepare(Request),
    /// Persist the irreversible source-local barrier.
    Barrier(Request),
    /// Persist the data retirement/cut/outbox while the journal remains RETIRING.
    Seal(Certificate),
    /// Complete the agreeing protected source journal.
    Retire(Certificate),
    /// Expose a usable certificate only after source retirement is durable.
    Publish(Certificate),
    /// Persist destination PENDING, bound to the inspected full artifact.
    Pending {
        certificate: Certificate,
        epoch: String,
        artifact: Digest,
    },
    /// Install the matching data/placement tuple; still no writer admission.
    Install {
        certificate: Certificate,
        artifact: Digest,
    },
    /// Serialize the external predecessor CAS after matching destination install.
    AdvanceWitness(Certificate),
    /// Atomically model receipt consumption plus ACTIVE/result journal write.
    Activate {
        certificate: Certificate,
        epoch: String,
        nonce: String,
        challenge: String,
    },
    /// Drop process admission, retaining every modeled durable record.
    Crash(Placement),
    /// Restart ONLY with agreeing records and a fresh externally current proof.
    Restart {
        authority: Authority,
        nonce: String,
        challenge: String,
    },
    /// Fixture-only old DB/sidecar restoration, including under a held handle.
    RestoreData {
        placement: Placement,
        data: Option<Authority>,
    },
    /// Fixture-only local host/journal rollback after the process has stopped.
    /// The external head and its nonce ledger are deliberately not rolled back.
    RestoreLocal {
        placement: Placement,
        journal: Option<Box<Journal>>,
        data: Option<Authority>,
    },
}

/// A model step's result. Historical activation replay does not start a process.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Outcome {
    /// State recorded or exact idempotent retry; no authority token issued.
    Recorded,
    /// Retained immutable activation result, possibly a historical readback.
    Activation(Box<Activation>),
}

impl Model {
    pub(super) fn step(
        &mut self,
        action: &Action,
        observations: Observations,
    ) -> ModelResult<Outcome> {
        match action {
            Action::Prepare(r) => {
                observations.admission()?;
                r.validate()?;
                if let Some(old) = self.operations.get(&r.operation) {
                    require(old == r, "operation ID reused with different payload")?;
                    return Ok(Outcome::Recorded);
                }
                require(
                    self.head == r.source && self.can_commit(&r.source),
                    "stale predecessor or unavailable source",
                )?;
                require(
                    !self.operations.values().any(|old| old.source == r.source),
                    "successor already reserved",
                )?;
                self.operations.insert(r.operation.clone(), r.clone());
            }
            Action::Barrier(r) => {
                observations.authorized()?;
                self.reserved(r)?;
                require(self.head == r.source, "stale predecessor")?;
                if self.journal.get(&r.source.owner)
                    == Some(&Journal::Retiring { request: r.clone() })
                {
                    return Ok(Outcome::Recorded);
                }
                require(self.can_commit(&r.source), "source is not admitted ACTIVE")?;
                self.journal.insert(
                    r.source.owner.clone(),
                    Journal::Retiring { request: r.clone() },
                );
            }
            Action::Seal(c) => {
                observations.authorized()?;
                c.validate()?;
                self.reserved(&c.request)?;
                require(
                    self.journal.get(&c.request.source.owner)
                        == Some(&Journal::Retiring {
                            request: c.request.clone(),
                        }),
                    "barrier not durable",
                )?;
                if let Some(old) = self.seals.get(&c.request.operation) {
                    require(old == c, "sealed cut changed on retry")?;
                }
                self.seals.insert(c.request.operation.clone(), c.clone());
            }
            Action::Retire(c) => {
                observations.authorized()?;
                c.validate()?;
                self.reserved(&c.request)?;
                let retired = Journal::Retired {
                    certificate: c.clone(),
                };
                if self.journal.get(&c.request.source.owner) == Some(&retired) {
                    return Ok(Outcome::Recorded);
                }
                require(
                    self.journal.get(&c.request.source.owner)
                        == Some(&Journal::Retiring {
                            request: c.request.clone(),
                        })
                        && self.seals.get(&c.request.operation) == Some(c),
                    "incomplete or contradictory retirement",
                )?;
                self.journal.insert(c.request.source.owner.clone(), retired);
            }
            Action::Publish(c) => {
                observations.authorized()?;
                c.validate()?;
                self.reserved(&c.request)?;
                if let Some(old) = self.published.get(&c.request.operation) {
                    require(old == c, "published certificate changed")?;
                    return Ok(Outcome::Recorded);
                }
                self.require_retired(c)?;
                require(self.head == c.request.source, "stale predecessor")?;
                self.published
                    .insert(c.request.operation.clone(), c.clone());
            }
            Action::Pending {
                certificate: c,
                epoch,
                artifact,
            } => {
                observations.admission()?;
                self.published(c)?;
                require(*artifact == c.cut.artifact, "wrong full artifact")?;
                let successor = c.successor(epoch)?;
                if let Some(old) = self.consumed.get(&c.request.operation) {
                    require(
                        old.certificate == *c && old.authority == successor,
                        "changed activation retry",
                    )?;
                    return Ok(Outcome::Activation(Box::new(old.clone())));
                }
                let pending = Journal::Pending {
                    certificate: c.clone(),
                    successor: successor.clone(),
                };
                if self.journal.get(&c.request.destination) == Some(&pending) {
                    return Ok(Outcome::Recorded);
                }
                require(self.head == c.request.source, "stale predecessor")?;
                require(!self.epochs.contains(epoch), "epoch already used")?;
                require(
                    match self.journal.get(&c.request.destination) {
                        None => true,
                        Some(Journal::Retired { certificate }) => {
                            certificate.request.source.generation < successor.generation
                        }
                        _ => false,
                    },
                    "destination has unresolved authority state",
                )?;
                self.epochs.insert(epoch.clone());
                self.journal.insert(c.request.destination.clone(), pending);
            }
            Action::Install {
                certificate: c,
                artifact,
            } => {
                observations.authorized()?;
                self.published(c)?;
                require(*artifact == c.cut.artifact, "artifact mismatch")?;
                let Some(Journal::Pending {
                    certificate,
                    successor,
                }) = self.journal.get(&c.request.destination)
                else {
                    return Err("destination not PENDING".into());
                };
                require(certificate == c, "different pending operation")?;
                self.data
                    .insert(c.request.destination.clone(), successor.clone());
            }
            Action::AdvanceWitness(c) => {
                observations.admission()?;
                self.published(c)?;
                let Some(Journal::Pending {
                    certificate,
                    successor,
                }) = self.journal.get(&c.request.destination)
                else {
                    return Err("destination not PENDING".into());
                };
                require(
                    certificate == c && self.data.get(&c.request.destination) == Some(successor),
                    "install is incomplete",
                )?;
                if self.head == *successor {
                    return Ok(Outcome::Recorded);
                }
                require(
                    self.head == c.request.source,
                    "witness predecessor CAS refused",
                )?;
                self.head = successor.clone();
            }
            Action::Activate {
                certificate: c,
                epoch,
                nonce,
                challenge,
            } => {
                observations.admission()?;
                self.published(c)?;
                let authority = c.successor(epoch)?;
                if let Some(old) = self.consumed.get(&c.request.operation) {
                    require(
                        old.certificate == *c && old.authority == authority,
                        "changed activation retry",
                    )?;
                    // Readback is NOT restart: never recreate process admission.
                    return Ok(Outcome::Activation(Box::new(old.clone())));
                }
                self.fresh_start(&authority, nonce, challenge, observations)?;
                require(
                    self.journal.get(&authority.owner)
                        == Some(&Journal::Pending {
                            certificate: c.clone(),
                            successor: authority.clone(),
                        })
                        && self.data.get(&authority.owner) == Some(&authority),
                    "destination records disagree",
                )?;
                let result = Activation {
                    certificate: c.clone(),
                    authority: authority.clone(),
                };
                self.nonces.insert(nonce.clone());
                self.journal.insert(
                    authority.owner.clone(),
                    Journal::Active {
                        authority: authority.clone(),
                    },
                );
                self.admitted.insert(authority.owner.clone(), authority);
                self.consumed
                    .insert(c.request.operation.clone(), result.clone());
                return Ok(Outcome::Activation(Box::new(result)));
            }
            Action::Crash(owner) => {
                self.admitted.remove(owner);
            }
            Action::Restart {
                authority,
                nonce,
                challenge,
            } => {
                self.fresh_start(authority, nonce, challenge, observations)?;
                require(
                    self.journal.get(&authority.owner)
                        == Some(&Journal::Active {
                            authority: authority.clone(),
                        })
                        && self.data.get(&authority.owner) == Some(authority),
                    "local state UNKNOWN or contradictory",
                )?;
                self.nonces.insert(nonce.clone());
                self.admitted
                    .insert(authority.owner.clone(), authority.clone());
            }
            Action::RestoreData { placement, data } => self.restore_data(placement, data)?,
            Action::RestoreLocal {
                placement,
                journal,
                data,
            } => {
                require(
                    !self.admitted.contains_key(placement),
                    "local authority rollback requires stopped process",
                )?;
                if let Some(journal) = journal {
                    Record {
                        version: RECORD_VERSION,
                        payload: Payload::Journal {
                            journal: journal.as_ref().clone(),
                        },
                    }
                    .validate()?;
                    self.journal
                        .insert(placement.clone(), journal.as_ref().clone());
                } else {
                    self.journal.remove(placement);
                }
                self.restore_data(placement, data)?;
            }
        }
        Ok(Outcome::Recorded)
    }

    fn restore_data(&mut self, placement: &Placement, data: &Option<Authority>) -> ModelResult<()> {
        if let Some(data) = data {
            data.validate()?;
            self.data.insert(placement.clone(), data.clone());
        } else {
            self.data.remove(placement);
        }
        Ok(())
    }
}
