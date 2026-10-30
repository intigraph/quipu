//! The signed frames of the scoped replication stream (`local-sync-mode.md`
//! §5), shared by the source that sends them and the applier that checks them.
//!
//! A stream session opens with a `session` frame and then carries `tx`
//! frames (one source transaction each, or a chunked one), `coverage` frames
//! and `control` frames. Every frame has a `frame_seq`, strictly increasing
//! from 0 within its session. The `session`, `coverage` and `control` frames
//! are signed by the source's stream key over [`message`]. New sources also
//! sign transaction bodies and chunk commits; strict-bootstrap clients require
//! those signatures. A coverage frame is
//! an assertion by its signer, never an inference from silence.
//!
//! The signed bytes are the domain tag [`DOMAIN`], the frame kind, then each
//! field as an 8-byte big-endian length followed by its UTF-8 bytes, so no
//! two field lists sign the same bytes. The tag keeps a stream signature from
//! verifying as any other envelope the same key signs (verdicts, shares).

/// The domain tag every stream signature starts with.
pub const DOMAIN: &str = "quipu-replicate/v1";

/// SSE event names of the stream's frames.
pub mod event {
    /// Opens a session: binds its id to the source, the scope and `since`.
    pub const SESSION: &str = "quipu.replicate.session";
    /// One whole source transaction.
    pub const TX: &str = "quipu.replicate.tx";
    /// The header of a chunked source transaction.
    pub const TX_BEGIN: &str = "quipu.replicate.tx.begin";
    /// One part of a chunked source transaction.
    pub const TX_PART: &str = "quipu.replicate.tx.part";
    /// The commit barrier of a chunked source transaction.
    pub const TX_COMMIT: &str = "quipu.replicate.tx.commit";
    /// A signed high-watermark: every in-scope transaction up to it was sent.
    pub const COVERAGE: &str = "quipu.replicate.coverage";
    /// Revocation, end of the epoch, or a protocol error.
    pub const CONTROL: &str = "quipu.replicate.control";
}

/// The fields a stream signature covers, in signing order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Signed<'a> {
    /// An authenticated transaction body (single frame or chunk commit).
    Transaction {
        /// Session id.
        session: &'a str,
        /// Persistent source store.
        store_id: &'a str,
        /// Source log epoch.
        log_epoch: &'a str,
        /// Pinned replication scope.
        scope: &'a str,
        /// Position of the transaction or commit frame.
        frame_seq: u64,
        /// Source transaction being applied.
        source_tx: i64,
        /// Digest of canonical records or the chunk chain.
        digest: &'a str,
        /// Distinguishes a chunk commit from a single-frame transaction.
        chunked: bool,
    },
    /// The opening frame of a session.
    Session {
        /// The session id.
        session: &'a str,
        /// The source store.
        store_id: &'a str,
        /// The source log epoch.
        log_epoch: &'a str,
        /// The scope digest the session serves.
        scope: &'a str,
        /// The cursor the session continues from.
        since: i64,
    },
    /// A coverage watermark, or the answer to a challenge.
    Coverage {
        /// The session id.
        session: &'a str,
        /// The source store.
        store_id: &'a str,
        /// The source log epoch.
        log_epoch: &'a str,
        /// The scope digest the session serves.
        scope: &'a str,
        /// The frame's position in the session.
        frame_seq: u64,
        /// Every in-scope source transaction at or below it has been sent.
        watermark: i64,
        /// The challenge nonce it answers (hex), or empty for a heartbeat.
        nonce: &'a str,
    },
    /// A control frame.
    Control {
        /// The session id.
        session: &'a str,
        /// The source store.
        store_id: &'a str,
        /// The source log epoch.
        log_epoch: &'a str,
        /// The scope digest the session serves.
        scope: &'a str,
        /// The frame's position in the session.
        frame_seq: u64,
        /// `revoked`, `epoch_end` or `error`.
        kind: &'a str,
        /// Why.
        reason: &'a str,
    },
}

/// The bytes a stream key signs for `frame`.
#[must_use]
pub fn message(frame: &Signed<'_>) -> Vec<u8> {
    let (kind, fields): (&str, Vec<String>) = match frame {
        Signed::Transaction {
            session,
            store_id,
            log_epoch,
            scope,
            frame_seq,
            source_tx,
            digest,
            chunked,
        } => (
            if *chunked {
                "transaction.commit"
            } else {
                "transaction"
            },
            vec![
                (*session).into(),
                (*store_id).into(),
                (*log_epoch).into(),
                (*scope).into(),
                frame_seq.to_string(),
                source_tx.to_string(),
                (*digest).into(),
            ],
        ),
        Signed::Session {
            session,
            store_id,
            log_epoch,
            scope,
            since,
        } => (
            "session",
            vec![
                (*session).into(),
                (*store_id).into(),
                (*log_epoch).into(),
                (*scope).into(),
                since.to_string(),
            ],
        ),
        Signed::Coverage {
            session,
            store_id,
            log_epoch,
            scope,
            frame_seq,
            watermark,
            nonce,
        } => (
            "coverage",
            vec![
                (*session).into(),
                (*store_id).into(),
                (*log_epoch).into(),
                (*scope).into(),
                frame_seq.to_string(),
                watermark.to_string(),
                (*nonce).into(),
            ],
        ),
        Signed::Control {
            session,
            store_id,
            log_epoch,
            scope,
            frame_seq,
            kind,
            reason,
        } => (
            "control",
            vec![
                (*session).into(),
                (*store_id).into(),
                (*log_epoch).into(),
                (*scope).into(),
                frame_seq.to_string(),
                (*kind).into(),
                (*reason).into(),
            ],
        ),
    };
    let mut out = Vec::new();
    for field in [DOMAIN, kind]
        .into_iter()
        .chain(fields.iter().map(String::as_str))
    {
        out.extend_from_slice(&(field.len() as u64).to_be_bytes());
        out.extend_from_slice(field.as_bytes());
    }
    out
}

/// Digest the complete ordered single-frame transaction body.
#[must_use]
pub fn records_digest(records: &[serde_json::Value]) -> String {
    use sha2::{Digest, Sha256};
    let encoded = canonical(&serde_json::Value::Array(records.to_vec()));
    hex::encode(Sha256::digest(encoded.as_bytes()))
}

fn canonical(value: &serde_json::Value) -> String {
    use serde_json::Value;
    match value {
        Value::Object(map) => {
            let mut keys: Vec<_> = map.keys().collect();
            keys.sort();
            format!(
                "{{{}}}",
                keys.into_iter()
                    .map(|key| {
                        format!("{}:{}", Value::String(key.clone()), canonical(&map[key]))
                    })
                    .collect::<Vec<_>>()
                    .join(",")
            )
        }
        Value::Array(items) => format!(
            "[{}]",
            items.iter().map(canonical).collect::<Vec<_>>().join(",")
        ),
        other => other.to_string(),
    }
}

/// Whether `signature_hex` is `public_key_hex`'s signature over `frame`.
#[must_use]
pub fn verify(public_key_hex: &str, frame: &Signed<'_>, signature_hex: &str) -> bool {
    crate::signing::verify_hex(public_key_hex, &message(frame), signature_hex)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fields_are_length_prefixed_so_boundaries_cannot_move() {
        let a = Signed::Session {
            session: "ab",
            store_id: "c",
            log_epoch: "e",
            scope: "s",
            since: 1,
        };
        let b = Signed::Session {
            session: "a",
            store_id: "bc",
            log_epoch: "e",
            scope: "s",
            since: 1,
        };
        assert_ne!(message(&a), message(&b));
    }

    #[test]
    fn a_heartbeat_and_a_challenge_answer_sign_different_bytes() {
        let at = |nonce| Signed::Coverage {
            session: "s",
            store_id: "st",
            log_epoch: "e",
            scope: "sc",
            frame_seq: 3,
            watermark: 9,
            nonce,
        };
        assert_ne!(message(&at("")), message(&at("00ff")));
    }

    #[test]
    fn the_signature_verifies_only_for_its_own_fields() {
        let doc = ring::signature::Ed25519KeyPair::generate_pkcs8(&ring::rand::SystemRandom::new())
            .unwrap();
        let key = ring::signature::Ed25519KeyPair::from_pkcs8(doc.as_ref()).unwrap();
        let public = crate::signing::public_key_hex(&key);
        let frame = Signed::Control {
            session: "s",
            store_id: "st",
            log_epoch: "e",
            scope: "sc",
            frame_seq: 4,
            kind: "revoked",
            reason: "grant withdrawn",
        };
        let signature = crate::signing::sign_hex(&key, &message(&frame));
        assert!(verify(&public, &frame, &signature));
        let other = Signed::Control {
            session: "s",
            store_id: "st",
            log_epoch: "e",
            scope: "sc",
            frame_seq: 5,
            kind: "revoked",
            reason: "grant withdrawn",
        };
        assert!(!verify(&public, &other, &signature));
    }
}
