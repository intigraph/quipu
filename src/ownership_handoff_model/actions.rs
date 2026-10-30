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
    /// Durably mark source retirement at the external witness BEFORE publication.
    FenceWitness(Certificate),
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
