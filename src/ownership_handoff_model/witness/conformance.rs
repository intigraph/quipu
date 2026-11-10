//! One conformance suite for every witness backend (design W2: "the same
//! suite as the in-memory adapter"). A backend supplies a fresh witness with
//! the assumed genesis and a fault injector mapping the deterministic W1
//! faults onto its own failure surface.
use super::*;
use crate::ownership_handoff_model::{AuthorityUnit, Cut, Placement};

pub(super) fn genesis() -> Authority {
    Authority {
        store_id: "urn:example:witness".into(),
        unit: AuthorityUnit::FullStore,
        generation: 0,
        epoch: "epoch-A".into(),
        owner: placement("A", 1),
        policy: "existing-policy".into(),
        predecessor: None,
    }
}

fn placement(name: &str, byte: u8) -> Placement {
    Placement {
        principal: name.into(),
        native: [byte; 32],
    }
}

fn certificate(source: Authority, destination: &str, op: &str) -> Certificate {
    Certificate {
        request: Request {
            operation: op.into(),
            operator: "existing-operator".into(),
            successor_generation: source.generation + 1,
            source,
            destination: placement(destination, 2),
            profile: "existing-profile".into(),
        },
        cut: Cut {
            transaction: 1,
            facts: [3; 32],
            artifact: [4; 32],
        },
    }
}

/// Canonical genesis bytes, for backends seeded from outside the witness.
pub(super) fn genesis_bytes() -> Vec<u8> {
    Entry::Active(genesis()).bytes().unwrap()
}

pub(super) trait Harness {
    type Backend: Objects + Sync;
    fn fresh(&self) -> ObjectWitness<Self::Backend>;
    /// Inject exactly one deterministic fault into the next operation.
    fn inject(&self, witness: &ObjectWitness<Self::Backend>, fault: Fault);
}

/// Run every shared scenario. Panics name the scenario.
pub(super) fn run<H: Harness>(h: &H) {
    let id = genesis().store_id;

    // Full lineage: ACTIVE -> RESERVED -> RETIRED -> ACTIVE(successor).
    let w = h.fresh();
    assert_eq!(
        w.head(&id, 0),
        Reply::Ok(WitnessHead::Active {
            authority: genesis()
        }),
        "genesis head"
    );
    let c = certificate(genesis(), "B", "op1");
    assert_eq!(w.reserve(0, &c.request), Reply::Ok(()), "reserve");
    assert_eq!(
        w.reserve(0, &c.request),
        Reply::Ok(()),
        "exact retry is idempotent"
    );
    assert_eq!(
        w.head(&id, 0),
        Reply::Ok(WitnessHead::Reserved {
            request: c.request.clone()
        }),
        "reserved head"
    );
    let other = certificate(genesis(), "C", "op2");
    assert_eq!(
        w.reserve(0, &other.request),
        Reply::Conflict,
        "second reserver loses"
    );
    assert_eq!(w.retire(0, &c), Reply::Ok(()), "retire");
    let next = c.successor("epoch-B").unwrap();
    assert_eq!(w.activate(&next), Reply::Ok(()), "activate successor");
    assert_eq!(
        w.head(&id, 0),
        Reply::Ok(WitnessHead::Active {
            authority: next.clone()
        }),
        "successor head from genesis anchor"
    );
    assert_eq!(
        w.head(&id, 1),
        Reply::Ok(WitnessHead::Active { authority: next }),
        "successor head from its own anchor"
    );
    assert_eq!(
        w.head("urn:example:other", 0),
        Reply::Conflict,
        "foreign lineage"
    );

    // UNKNOWN is never an advance, and the exact retry reads back.
    let w = h.fresh();
    h.inject(&w, Fault::Unknown);
    assert_eq!(w.reserve(0, &c.request), Reply::Unknown, "injected unknown");
    assert_eq!(
        w.head(&id, 0),
        Reply::Ok(WitnessHead::Active {
            authority: genesis()
        }),
        "unknown did not write"
    );
    h.inject(&w, Fault::Slow);
    assert_eq!(w.head(&id, 0), Reply::Unknown, "slow witness is unknown");

    // A lost reply after a committed create: UNKNOWN now, exact readback later.
    let w = h.fresh();
    h.inject(&w, Fault::LostReply);
    assert_eq!(w.reserve(0, &c.request), Reply::Unknown, "lost reply");
    assert_eq!(
        w.head(&id, 0),
        Reply::Ok(WitnessHead::Reserved {
            request: c.request.clone()
        }),
        "lost reply did commit"
    );
    assert_eq!(
        w.reserve(0, &c.request),
        Reply::Ok(()),
        "exact retry after lost reply"
    );
    assert_eq!(
        w.reserve(0, &other.request),
        Reply::Conflict,
        "different body still loses"
    );

    // Concurrent different reservers: exactly one winner, from the key alone.
    let w = h.fresh();
    // UNKNOWN (for example lock contention) is retried exactly, as a real
    // caller must; it is never counted as a win.
    let until_known = |request: &Request| {
        for _ in 0..200 {
            match w.reserve(0, request) {
                Reply::Unknown => std::thread::yield_now(),
                known => return known,
            }
        }
        Reply::Unknown
    };
    let results: Vec<_> = std::thread::scope(|s| {
        let a = s.spawn(|| until_known(&c.request));
        let b = s.spawn(|| until_known(&other.request));
        vec![a.join().unwrap(), b.join().unwrap()]
    });
    assert_eq!(
        results.iter().filter(|r| **r == Reply::Ok(())).count(),
        1,
        "exactly one concurrent winner: {results:?}"
    );
    assert_eq!(
        results.iter().filter(|r| **r == Reply::Conflict).count(),
        1,
        "the loser sees a conflict, not silence: {results:?}"
    );
}
