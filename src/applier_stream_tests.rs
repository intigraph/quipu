//! The scoped replication stream against the applier: sessions, sequence,
//! signed coverage and the freshness challenge (`local-sync-mode.md` §5 and
//! the P2 controls of §12). Frames come from a test producer that signs
//! exactly as the source does, so each control below fails if its check is
//! removed from `applier_stream.rs`.
use ring::signature::Ed25519KeyPair;
use serde_json::{Value as Json, json};
use sha2::{Digest, Sha256};

use super::super::tests::source;
use super::super::*;
use crate::replicate_wire::{Signed, event, message};
use crate::signing::{public_key_hex, sign_hex};
use crate::store::changes::Capture;

const SCOPE: &str = "scope-digest-under-test";

fn keypair() -> Ed25519KeyPair {
    let doc = Ed25519KeyPair::generate_pkcs8(&ring::rand::SystemRandom::new()).unwrap();
    Ed25519KeyPair::from_pkcs8(doc.as_ref()).unwrap()
}

/// Builds one session's frames the way the source does: every frame numbered,
/// session, coverage and control frames signed by `key`.
struct Producer<'a> {
    src: &'a Store,
    key: &'a Ed25519KeyPair,
    session: String,
    seq: u64,
    since: i64,
}

impl<'a> Producer<'a> {
    fn new(src: &'a Store, key: &'a Ed25519KeyPair, session: &str) -> Self {
        Self {
            src,
            key,
            session: session.into(),
            seq: 0,
            since: 0,
        }
    }

    fn source(&self) -> Json {
        json!({"store_id": self.src.store_id().unwrap(), "log_epoch": self.src.log_epoch().unwrap()})
    }

    fn open(&mut self, since: i64) -> (&'static str, String) {
        self.seq = 0;
        self.since = since;
        let source = self.source();
        let signed = Signed::Session {
            session: &self.session,
            store_id: source["store_id"].as_str().unwrap(),
            log_epoch: source["log_epoch"].as_str().unwrap(),
            scope: SCOPE,
            since,
        };
        let signature = sign_hex(self.key, &message(&signed));
        let data = json!({"session": self.session, "frame_seq": 0, "source": source,
                          "scope": SCOPE, "since": since, "signature": signature});
        (event::SESSION, data.to_string())
    }

    fn next(&mut self) -> u64 {
        self.seq += 1;
        self.seq
    }

    /// Every transaction after `since`, one `tx` frame each.
    fn txs(&mut self, since: i64) -> Vec<(&'static str, String)> {
        let mut out = Vec::new();
        let mut cursor = since;
        loop {
            let page = self
                .src
                .changes_after(cursor, 1, Capture::OldAndNewValues, None)
                .unwrap();
            if page.records.is_empty() {
                return out;
            }
            cursor = page.next_tx;
            let seq = self.next();
            let data = json!({"session": self.session, "frame_seq": seq, "source": self.source(),
                              "source_tx": cursor, "records": page.records});
            out.push((event::TX, data.to_string()));
        }
    }

    fn coverage(&mut self, watermark: i64, nonce: Option<[u8; 16]>) -> (&'static str, String) {
        let seq = self.next();
        let nonce = nonce.map(hex::encode).unwrap_or_default();
        let source = self.source();
        let signed = Signed::Coverage {
            session: &self.session,
            store_id: source["store_id"].as_str().unwrap(),
            log_epoch: source["log_epoch"].as_str().unwrap(),
            scope: SCOPE,
            frame_seq: seq,
            watermark,
            nonce: &nonce,
        };
        let signature = sign_hex(self.key, &message(&signed));
        let nonce = if nonce.is_empty() {
            Json::Null
        } else {
            json!(nonce)
        };
        let data = json!({"session": self.session, "frame_seq": seq, "source": source,
                          "watermark": watermark, "nonce": nonce, "signature": signature});
        (event::COVERAGE, data.to_string())
    }

    fn control(&mut self, kind: &str, reason: &str) -> (&'static str, String) {
        let seq = self.next();
        let source = self.source();
        let signed = Signed::Control {
            session: &self.session,
            store_id: source["store_id"].as_str().unwrap(),
            log_epoch: source["log_epoch"].as_str().unwrap(),
            scope: SCOPE,
            frame_seq: seq,
            kind,
            reason,
        };
        let signature = sign_hex(self.key, &message(&signed));
        let data = json!({"session": self.session, "frame_seq": seq, "source": source,
                          "kind": kind, "reason": reason, "signature": signature});
        (event::CONTROL, data.to_string())
    }

    /// The transaction after `since` as begin, parts and commit.
    fn chunked(&mut self, since: i64, per_part: usize) -> Vec<(&'static str, String)> {
        let page = self
            .src
            .changes_after(since, 1, Capture::OldAndNewValues, None)
            .unwrap();
        let tx = page.next_tx;
        let records = page.records;
        let begin_seq = self.next();
        let mut hasher = Sha256::new();
        let mut parts = Vec::new();
        for (index, chunk) in records.chunks(per_part).enumerate() {
            let before = hex::encode(hasher.clone().finalize());
            let seq = self.next();
            let data = json!({"session": self.session, "frame_seq": seq, "source_tx": tx,
                              "index": index, "digest_before": before, "records": chunk})
            .to_string();
            hasher.update(data.as_bytes());
            parts.push((event::TX_PART, data));
        }
        let digest = hex::encode(hasher.finalize());
        let mut frames = vec![(
            event::TX_BEGIN,
            json!({"session": self.session, "frame_seq": begin_seq, "source_tx": tx,
                   "parts": parts.len(), "records": records.len(), "digest": digest,
                   "since": self.since})
            .to_string(),
        )];
        frames.extend(parts);
        let seq = self.next();
        frames.push((
            event::TX_COMMIT,
            json!({"session": self.session, "frame_seq": seq, "source_tx": tx,
                   "digest": digest, "since": self.since, "source": self.source()})
            .to_string(),
        ));
        frames
    }
}

fn head(src: &Store) -> i64 {
    src.changes_after(0, usize::MAX >> 1, Capture::OldAndNewValues, None)
        .unwrap()
        .watermark_tx
}

fn replica(key: &Ed25519KeyPair) -> (Store, Applier) {
    let store = Store::open_in_memory().unwrap();
    let mut applier = Applier::open(&store).unwrap();
    applier.trust_stream(&public_key_hex(key), SCOPE);
    (store, applier)
}

fn feed(
    store: &mut Store,
    applier: &mut Applier,
    frames: &[(&str, String)],
    now: u64,
) -> Vec<Outcome> {
    frames
        .iter()
        .map(|(e, d)| applier.apply_frame(store, e, d, now).unwrap())
        .collect()
}

fn one(store: &mut Store, applier: &mut Applier, frame: &(&str, String), now: u64) -> Outcome {
    applier.apply_frame(store, frame.0, &frame.1, now).unwrap()
}

/// A replica that has followed the whole source through one session.
fn caught_up<'a>(src: &'a Store, key: &'a Ed25519KeyPair) -> (Store, Applier, Producer<'a>) {
    let (mut store, mut applier) = replica(key);
    let mut p = Producer::new(src, key, "s1");
    let mut frames = vec![p.open(0)];
    frames.extend(p.txs(0));
    frames.push(p.coverage(head(src), None));
    feed(&mut store, &mut applier, &frames, 1);
    (store, applier, p)
}

#[test]
fn a_scoped_follower_fed_only_by_the_stream_equals_its_source() {
    let src = source();
    let key = keypair();
    let (mut store, mut applier) = replica(&key);
    let mut p = Producer::new(&src, &key, "s1");
    assert_eq!(
        one(&mut store, &mut applier, &p.open(0), 1),
        Outcome::SessionOpened {
            session: "s1".into()
        }
    );
    for o in feed(&mut store, &mut applier, &p.txs(0), 1) {
        assert!(matches!(o, Outcome::Applied { .. }), "{o:?}");
    }
    let w = head(&src);
    assert_eq!(
        one(&mut store, &mut applier, &p.coverage(w, None), 1),
        Outcome::CoverageAdvanced { watermark: w }
    );
    assert_eq!(live_digest(&store), live_digest(&src));
    assert_eq!(applier.coverage(), Some(w));
}

#[test]
fn a_chunked_transaction_through_the_stream_is_invisible_until_its_barrier() {
    let src = source();
    let key = keypair();
    let (mut store, mut applier) = replica(&key);
    let mut p = Producer::new(&src, &key, "s1");
    one(&mut store, &mut applier, &p.open(0), 1);
    let frames = p.chunked(0, 1);
    let (body, commit) = frames.split_at(frames.len() - 1);
    for o in feed(&mut store, &mut applier, body, 1) {
        assert!(matches!(o, Outcome::Buffered { .. }), "{o:?}");
    }
    assert!(live_digest(&store).is_empty());
    assert!(matches!(
        one(&mut store, &mut applier, &commit[0], 1),
        Outcome::Applied { .. }
    ));
    assert!(!live_digest(&store).is_empty());
}

#[test]
fn a_frame_sequence_gap_drops_the_session_and_resumes_with_nothing_applied() {
    let src = source();
    let key = keypair();
    let (mut store, mut applier) = replica(&key);
    let mut p = Producer::new(&src, &key, "s1");
    one(&mut store, &mut applier, &p.open(0), 1);
    let txs = p.txs(0);
    one(&mut store, &mut applier, &txs[0], 1);
    let before = live_digest(&store);
    let cursor = applier.progress().unwrap().source_tx;
    // txs[1] is lost: txs[2] is out of sequence.
    assert_eq!(
        one(&mut store, &mut applier, &txs[2], 1),
        Outcome::Resume { from: cursor }
    );
    assert_eq!(live_digest(&store), before);
    assert_eq!(applier.session(), None);
    // The rest of the old session is refused too.
    assert_eq!(
        one(&mut store, &mut applier, &txs[1], 1),
        Outcome::Resume { from: cursor }
    );
}

#[test]
fn a_transaction_older_than_the_session_has_sent_is_a_reorder() {
    let src = source();
    let key = keypair();
    let (mut store, mut applier) = replica(&key);
    let mut p = Producer::new(&src, &key, "s1");
    one(&mut store, &mut applier, &p.open(0), 1);
    let txs = p.txs(0);
    feed(&mut store, &mut applier, &txs[..2], 1);
    // The first transaction again, correctly numbered as the next frame.
    let mut again: Json = serde_json::from_str(&txs[0].1).unwrap();
    again["frame_seq"] = json!(3);
    let o = one(&mut store, &mut applier, &(event::TX, again.to_string()), 1);
    assert!(matches!(o, Outcome::Resume { .. }), "{o:?}");
}

#[test]
fn a_new_session_redelivering_applied_transactions_checks_them_as_duplicates() {
    let src = source();
    let key = keypair();
    let (mut store, mut applier, _) = caught_up(&src, &key);
    let mut p = Producer::new(&src, &key, "s2");
    one(&mut store, &mut applier, &p.open(0), 2);
    for o in feed(&mut store, &mut applier, &p.txs(0), 2) {
        assert_eq!(o, Outcome::Duplicate);
    }
    assert_eq!(live_digest(&store), live_digest(&src));
}

#[test]
fn a_session_that_starts_past_the_cursor_resumes() {
    let src = source();
    let key = keypair();
    let (mut store, mut applier) = replica(&key);
    let mut p = Producer::new(&src, &key, "s1");
    assert_eq!(
        one(&mut store, &mut applier, &p.open(2), 1),
        Outcome::Resume { from: 0 }
    );
    assert_eq!(applier.session(), None);
}

#[test]
fn signed_frames_are_refused_until_a_stream_key_is_pinned() {
    let src = source();
    let key = keypair();
    let mut store = Store::open_in_memory().unwrap();
    let mut applier = Applier::open(&store).unwrap();
    let mut p = Producer::new(&src, &key, "s1");
    assert!(matches!(
        one(&mut store, &mut applier, &p.open(0), 1),
        Outcome::Refused { .. }
    ));
}

#[test]
fn a_session_signed_by_another_key_is_refused() {
    let src = source();
    let key = keypair();
    let other = keypair();
    let (mut store, mut applier) = replica(&key);
    let mut p = Producer::new(&src, &other, "s1");
    assert!(matches!(
        one(&mut store, &mut applier, &p.open(0), 1),
        Outcome::Refused { .. }
    ));
    assert_eq!(applier.session(), None);
}

#[test]
fn unsigned_or_wrongly_signed_coverage_is_not_coverage() {
    let src = source();
    let key = keypair();
    let other = keypair();
    let (mut store, mut applier, mut p) = caught_up(&src, &key);
    let w = applier.coverage().unwrap();
    // The same session's next frame, signed by another key.
    let mut forged = Producer::new(&src, &other, "s1");
    forged.seq = p.seq;
    assert!(matches!(
        one(&mut store, &mut applier, &forged.coverage(w + 5, None), 2),
        Outcome::Resume { .. }
    ));
    assert_eq!(applier.coverage(), Some(w));
    // And with the signature removed.
    let (mut store, mut applier, _) = caught_up(&src, &key);
    let (e, data) = p.coverage(w + 5, None);
    let mut unsigned: Json = serde_json::from_str(&data).unwrap();
    unsigned.as_object_mut().unwrap().remove("signature");
    assert!(matches!(
        one(&mut store, &mut applier, &(e, unsigned.to_string()), 2),
        Outcome::Resume { .. }
    ));
    assert_eq!(applier.coverage(), Some(w));
}

#[test]
fn coverage_never_moves_backwards() {
    let src = source();
    let key = keypair();
    let (mut store, mut applier, mut p) = caught_up(&src, &key);
    let w = applier.coverage().unwrap();
    assert!(matches!(
        one(&mut store, &mut applier, &p.coverage(w - 1, None), 2),
        Outcome::Resume { .. }
    ));
    assert_eq!(applier.coverage(), Some(w));
    // A later session from an older cursor has sent nothing past it yet, but
    // still cannot take back what an earlier session covered.
    let mut later = Producer::new(&src, &key, "s2");
    one(&mut store, &mut applier, &later.open(0), 3);
    assert!(matches!(
        one(&mut store, &mut applier, &later.coverage(w - 1, None), 3),
        Outcome::Resume { .. }
    ));
    assert_eq!(applier.coverage(), Some(w));
}

#[test]
fn a_filtered_no_op_advances_coverage_past_the_last_applied_transaction() {
    let src = source();
    let key = keypair();
    let (mut store, mut applier, mut p) = caught_up(&src, &key);
    let applied = applier.progress().unwrap().source_tx;
    // The source committed out-of-scope transactions up to applied + 7.
    assert_eq!(
        one(&mut store, &mut applier, &p.coverage(applied + 7, None), 2),
        Outcome::CoverageAdvanced {
            watermark: applied + 7
        }
    );
    // A reconnect continues from the coverage, not from the last tx.
    let mut next = Producer::new(&src, &key, "s2");
    assert!(matches!(
        one(&mut store, &mut applier, &next.open(applied + 7), 3),
        Outcome::SessionOpened { .. }
    ));
}

#[test]
fn a_fresh_challenge_answer_bounds_staleness_by_its_send_time() {
    let src = source();
    let key = keypair();
    let (mut store, mut applier, mut p) = caught_up(&src, &key);
    let w = applier.coverage().unwrap();
    assert_eq!(applier.freshness(1_000).staleness_bound_ms, None);
    applier.challenge("s1", [7; 16], 1_000).unwrap();
    assert_eq!(
        one(
            &mut store,
            &mut applier,
            &p.coverage(w, Some([7; 16])),
            1_040
        ),
        Outcome::ProofAccepted { t_send_ms: 1_000 }
    );
    let f = applier.freshness(1_050);
    assert_eq!(f.staleness_bound_ms, Some(50));
    assert!(f.bounded && f.within(100));
    // The same proof, read later, bounds less tightly.
    assert!(!applier.freshness(1_500).within(100));
}

#[test]
fn a_heartbeat_delayed_sixty_seconds_never_satisfies_a_bounded_read() {
    let src = source();
    let key = keypair();
    let (mut store, mut applier, mut p) = caught_up(&src, &key);
    let w = applier.coverage().unwrap();
    // A valid heartbeat, signed at the source, arrives 60 s late, in order.
    let late = p.coverage(w, None);
    assert_eq!(
        one(&mut store, &mut applier, &late, 61_000),
        Outcome::CoverageAdvanced { watermark: w }
    );
    let f = applier.freshness(61_000);
    assert_eq!(f.staleness_bound_ms, None);
    assert!(!f.within(100));
}

#[test]
fn a_quiet_source_passes_by_challenge_with_no_commits() {
    let src = source();
    let key = keypair();
    let (mut store, mut applier, mut p) = caught_up(&src, &key);
    let w = applier.coverage().unwrap();
    // An hour with no commits: only the challenge answer arrives.
    applier.challenge("s1", [1; 16], 3_600_000).unwrap();
    one(
        &mut store,
        &mut applier,
        &p.coverage(w, Some([1; 16])),
        3_600_020,
    );
    assert!(applier.freshness(3_600_030).within(100));
}

#[test]
fn a_replayed_or_unsolicited_nonce_is_not_a_proof() {
    let src = source();
    let key = keypair();
    let (mut store, mut applier, mut p) = caught_up(&src, &key);
    let w = applier.coverage().unwrap();
    // Unsolicited: the source echoes a nonce nobody issued.
    assert_eq!(
        one(&mut store, &mut applier, &p.coverage(w, Some([9; 16])), 10),
        Outcome::CoverageAdvanced { watermark: w }
    );
    assert_eq!(applier.freshness(10).staleness_bound_ms, None);
    // Replayed: a nonce is consumed by its first answer and cannot be reissued.
    applier.challenge("s1", [2; 16], 10).unwrap();
    one(&mut store, &mut applier, &p.coverage(w, Some([2; 16])), 20);
    assert!(applier.challenge("s1", [2; 16], 30).is_err());
    applier.clock_discontinuity();
    assert_eq!(
        one(&mut store, &mut applier, &p.coverage(w, Some([2; 16])), 40),
        Outcome::CoverageAdvanced { watermark: w }
    );
    assert_eq!(applier.freshness(40).staleness_bound_ms, None);
}

#[test]
fn a_challenge_on_another_session_is_refused() {
    let src = source();
    let key = keypair();
    let (_store, mut applier, _p) = caught_up(&src, &key);
    assert!(applier.challenge("s0", [3; 16], 1).is_err());
}

#[test]
fn proofs_end_with_a_restart_a_clock_discontinuity_or_the_session() {
    let src = source();
    let key = keypair();
    let proven = |src, key| {
        let (mut store, mut applier, mut p) = caught_up(src, key);
        let w = applier.coverage().unwrap();
        applier.challenge("s1", [4; 16], 100).unwrap();
        one(&mut store, &mut applier, &p.coverage(w, Some([4; 16])), 110);
        assert!(applier.freshness(120).within(100));
        (store, applier)
    };
    let (store, mut applier) = proven(&src, &key);
    applier.clock_discontinuity();
    assert_eq!(applier.freshness(120).staleness_bound_ms, None);

    let (_store, mut applier) = proven(&src, &key);
    applier.end_session();
    assert_eq!(applier.freshness(120).staleness_bound_ms, None);

    // Reopened: the coverage survives, the proof does not.
    let w = applier.coverage();
    drop(applier);
    let reopened = Applier::open(&store).unwrap();
    assert_eq!(reopened.coverage(), w);
    assert_eq!(reopened.freshness(120).staleness_bound_ms, None);
}

#[test]
fn a_disconnected_follower_reports_unknown_freshness() {
    let src = source();
    let key = keypair();
    let (mut store, mut applier, mut p) = caught_up(&src, &key);
    let w = applier.coverage().unwrap();
    applier.challenge("s1", [5; 16], 0).unwrap();
    one(&mut store, &mut applier, &p.coverage(w, Some([5; 16])), 5);
    // The transport loses the connection; no challenge can be answered.
    applier.end_session();
    let f = applier.freshness(10);
    assert_eq!(f.staleness_bound_ms, None);
    assert!(!f.bounded);
}

#[test]
fn a_revocation_stops_the_replica_across_reopening() {
    let src = source();
    let key = keypair();
    let (mut store, mut applier, mut p) = caught_up(&src, &key);
    let revoked = Outcome::Revoked {
        reason: "grant withdrawn".into(),
    };
    assert_eq!(
        one(
            &mut store,
            &mut applier,
            &p.control("revoked", "grant withdrawn"),
            2
        ),
        revoked
    );
    let mut next = Producer::new(&src, &key, "s2");
    assert_eq!(one(&mut store, &mut applier, &next.open(0), 3), revoked);
    let mut reopened = Applier::open(&store).unwrap();
    reopened.trust_stream(&public_key_hex(&key), SCOPE);
    assert_eq!(one(&mut store, &mut reopened, &next.open(0), 4), revoked);
    assert_eq!(reopened.freshness(4).staleness_bound_ms, None);
}

#[test]
fn a_revocation_mid_chunk_discards_the_stage() {
    let src = source();
    let key = keypair();
    let (mut store, mut applier) = replica(&key);
    let mut p = Producer::new(&src, &key, "s1");
    one(&mut store, &mut applier, &p.open(0), 1);
    let frames = p.chunked(0, 1);
    feed(&mut store, &mut applier, &frames[..2], 1);
    // The producer's numbering already counted the chunk; revoke after part 0.
    p.seq = 2;
    assert!(matches!(
        one(
            &mut store,
            &mut applier,
            &p.control("revoked", "grant withdrawn"),
            1
        ),
        Outcome::Revoked { .. }
    ));
    assert!(live_digest(&store).is_empty());
    assert!(applier.progress().is_none());
}

#[test]
fn the_end_of_the_source_epoch_asks_for_a_new_bootstrap() {
    let src = source();
    let key = keypair();
    let (mut store, mut applier, mut p) = caught_up(&src, &key);
    assert!(matches!(
        one(
            &mut store,
            &mut applier,
            &p.control("epoch_end", "deep freeze"),
            2
        ),
        Outcome::Rebootstrap { .. }
    ));
}

#[test]
fn a_session_for_another_scope_asks_for_a_new_bootstrap() {
    let src = source();
    let key = keypair();
    let store = Store::open_in_memory().unwrap();
    let mut store = store;
    let mut applier = Applier::open(&store).unwrap();
    applier.trust_stream(&public_key_hex(&key), "another-scope");
    let mut p = Producer::new(&src, &key, "s1");
    assert!(matches!(
        one(&mut store, &mut applier, &p.open(0), 1),
        Outcome::Rebootstrap { .. }
    ));
}
