use super::*;
use std::collections::{BTreeMap, BTreeSet};

/// Oracle observations for this model ONLY. They are not verification results
/// and must never be used as a production authentication/authorization API.
#[derive(Clone, Copy, Debug, Default)]
pub struct Observations {
    /// Existing profile authenticated the exact request/proof body.
    pub authenticated: bool,
    /// Operator, source and destination still have current transition grants.
    pub current_grants: bool,
    /// A current, non-rolled-back external witness is reachable.
    pub witness_online: bool,
}

impl Observations {
    pub(super) fn authorized(self) -> ModelResult<()> {
        require(
            self.authenticated && self.current_grants,
            "authority UNKNOWN or revoked",
        )
    }

    pub(super) fn admission(self) -> ModelResult<()> {
        self.authorized()?;
        require(self.witness_online, "current witness unavailable")
    }
}

/// An executable specification with abstract durable tables. It performs no I/O.
/// Each action is one atomic model step; crash points BETWEEN resources are
/// separate actions, not a claim that real journal/data fsyncs are atomic.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct Model {
    pub(super) head: Authority,
    pub(super) journal: BTreeMap<Placement, Journal>,
    pub(super) data: BTreeMap<Placement, Authority>,
    pub(super) operations: BTreeMap<String, Request>,
    pub(super) seals: BTreeMap<String, Certificate>,
    pub(super) published: BTreeMap<String, Certificate>,
    pub(super) consumed: BTreeMap<String, Activation>,
    pub(super) epochs: BTreeSet<String>,
    pub(super) nonces: BTreeSet<String>,
    // Process admission is volatile; a crash never restores it from local bytes.
    pub(super) admitted: BTreeMap<Placement, Authority>,
}

impl Model {
    /// Start from an ASSUMED already enrolled/admitted generation-zero owner.
    /// Mid-chain recovery must retain history, never reseed this constructor.
    /// This does not enroll, sign, provision or open an actual store.
    pub fn new(authority: Authority) -> ModelResult<Self> {
        authority.validate()?;
        require(
            authority.generation == 0,
            "cannot seed a model without prior authority history",
        )?;
        let owner = authority.owner.clone();
        Ok(Self {
            head: authority.clone(),
            journal: BTreeMap::from([(
                owner.clone(),
                Journal::Active {
                    authority: authority.clone(),
                },
            )]),
            data: BTreeMap::from([(owner.clone(), authority.clone())]),
            admitted: BTreeMap::from([(owner, authority.clone())]),
            epochs: BTreeSet::from([authority.epoch]),
            operations: BTreeMap::new(),
            seals: BTreeMap::new(),
            published: BTreeMap::new(),
            consumed: BTreeMap::new(),
            nonces: BTreeSet::new(),
        })
    }

    /// Current external head in the model; reading it is not witness contact.
    pub fn head(&self) -> &Authority {
        &self.head
    }

    /// Inspect a role. Absence is UNKNOWN for this enrolled unit, never INACTIVE.
    pub fn journal(&self, owner: &Placement) -> Option<&Journal> {
        self.journal.get(owner)
    }

    /// Test a held handle against current LOCAL records and process admission.
    /// There is deliberately no witness RPC: an admitted running A works offline,
    /// but its journal barrier must stop it before another owner can be admitted.
    pub fn can_commit(&self, handle: &Authority) -> bool {
        self.admitted.get(&handle.owner) == Some(handle)
            && self.data.get(&handle.owner) == Some(handle)
            && self.journal.get(&handle.owner)
                == Some(&Journal::Active {
                    authority: handle.clone(),
                })
    }

    /// Placements that could currently commit, not merely stale ACTIVE files.
    pub fn writable(&self) -> Vec<&Authority> {
        self.admitted
            .values()
            .filter(|a| self.can_commit(a))
            .collect()
    }

    /// Apply atomically or leave the model unchanged on refusal. A successful
    /// action is a modeled state step, never evidence of physical durability.
    pub fn apply(&mut self, action: &Action, observations: Observations) -> ModelResult<Outcome> {
        let mut next = self.clone();
        let outcome = next.step(action, observations)?;
        *self = next;
        Ok(outcome)
    }

    pub(super) fn reserved(&self, request: &Request) -> ModelResult<()> {
        request.validate()?;
        require(
            self.operations.get(&request.operation) == Some(request),
            "operation not reserved or payload changed",
        )
    }

    pub(super) fn published(&self, certificate: &Certificate) -> ModelResult<()> {
        certificate.validate()?;
        require(
            self.published.get(&certificate.request.operation) == Some(certificate),
            "retirement not published or payload changed",
        )
    }

    // Critical usable-receipt boundary. The retained source mutant removes THIS
    // check; the model explorer must then find two writable placements.
    pub(super) fn require_retired(&self, certificate: &Certificate) -> ModelResult<()> {
        require(
            self.journal.get(&certificate.request.source.owner)
                == Some(&Journal::Retired {
                    certificate: certificate.clone(),
                })
                && self.seals.get(&certificate.request.operation) == Some(certificate),
            "source retirement and sealed durable outbox do not agree",
        )
    }

    pub(super) fn fresh_start(
        &self,
        authority: &Authority,
        nonce: &str,
        challenge: &str,
        observations: Observations,
    ) -> ModelResult<()> {
        observations.admission()?;
        Record {
            version: RECORD_VERSION,
            payload: Payload::Startup {
                authority: authority.clone(),
                nonce: nonce.into(),
            },
        }
        .validate()?;
        require(
            nonce == challenge && !self.nonces.contains(nonce),
            "stale or mismatched startup nonce",
        )?;
        require(
            self.head == *authority,
            "startup proof is not the current external head",
        )
    }
}
