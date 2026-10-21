//! Challenge issuance, verification and read-only settlement recovery.

use ring::rand::{SecureRandom as _, SystemRandom};

use super::*;

impl Store {
    /// Issue one random, expiring challenge for one exact request revision,
    /// declared choice and active credential. Uses only the server clock.
    pub fn deck_present(
        &mut self,
        request: &str,
        revision: u32,
        credential: &str,
        choice: &str,
    ) -> Result<Challenge> {
        let actor = self.deck_scope(true)?.actor;
        self.deck_atomic(&actor, |store, _| {
            let config = store.deck_scope(true)?;
            store.deck_credential(credential)?;
            let request = store.deck_request(request)?;
            let time = now()?;
            if request.revision != revision
                || request.expires_at <= time
                || !request.choices.iter().any(|c| c == choice)
            {
                return Err(refuse("stale, expired request or undeclared choice"));
            }
            let answered: bool = store.conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM deck_receipts WHERE request=?1 AND revision=?2)",
                params![request.id, revision],
                |r| r.get(0),
            )?;
            if answered {
                return Err(refuse("request revision already decided"));
            }
            let mut bytes = [0u8; 32];
            SystemRandom::new()
                .fill(&mut bytes)
                .map_err(|_| refuse("nonce entropy unavailable"))?;
            let challenge = Challenge {
                domain: "quipu-decision-deck-v1".into(),
                audience: store.store_id()?,
                nonce: hex::encode(bytes),
                origin: config.origin,
                rp_id: config.rp_id,
                actor: config.actor,
                credential: credential.into(),
                choice: choice.into(),
                expires_at: time
                    .checked_add(300)
                    .ok_or_else(|| refuse("clock overflow"))?
                    .min(request.expires_at),
                request,
            };
            store.conn.execute(
                "INSERT INTO deck_challenges(nonce,data) VALUES(?1,?2)",
                params![challenge.nonce, encode(&challenge)?],
            )?;
            Ok((challenge, Vec::new()))
        })
    }

    /// Verify the stored action and atomically consume its challenge, record
    /// one decision for that request/version and advance the credential counter.
    /// A repeated submission is refused even if its first response was lost.
    pub fn deck_attest(
        &mut self,
        nonce: &str,
        assertion: &webauthn::Assertion<'_>,
    ) -> Result<Receipt> {
        if nonce.len() != 64
            || !nonce
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || assertion.client_data_json.len() > 16384
            || assertion.authenticator_data.len() > 16384
            || assertion.signature.len() > 512
        {
            return Err(refuse("invalid nonce or oversized assertion"));
        }
        let actor = self.deck_scope(true)?.actor;
        self.deck_atomic(&actor, |store, ts| {
            let config = store.deck_scope(true)?;
            let row: Option<(Vec<u8>, bool)> = store
                .conn
                .query_row(
                    "SELECT data,consumed FROM deck_challenges WHERE nonce=?1",
                    [nonce],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?;
            let (data, consumed) = row.ok_or_else(|| refuse("unknown challenge"))?;
            if consumed {
                return Err(refuse("replayed challenge"));
            }
            let challenge: Challenge = decode(&data)?;
            let time = now()?;
            if challenge.expires_at <= time
                || challenge.request.expires_at <= time
                || challenge.audience != store.store_id()?
                || challenge.origin != config.origin
                || challenge.rp_id != config.rp_id
                || challenge.actor != config.actor
                || challenge.request != store.deck_request(&challenge.request.id)?
            {
                return Err(refuse("expired challenge, scope mismatch or stale request"));
            }
            let (credential, count) = store.deck_credential(&challenge.credential)?;
            // Typed deserialization rejects duplicate fields and malformed
            // crossOrigin rather than letting Value::as_bool treat them as false.
            #[derive(Deserialize)]
            struct ClientData {
                #[serde(rename = "type")]
                kind: String,
                challenge: String,
                origin: String,
                #[serde(default, rename = "crossOrigin")]
                cross_origin: bool,
            }
            let client: ClientData = decode(assertion.client_data_json)?;
            if client.cross_origin
                || client.kind != "webauthn.get"
                || client.origin != config.origin
                || client.challenge.is_empty()
            {
                return Err(refuse("invalid client data or cross-origin assertion"));
            }
            let scheme =
                Scheme::parse(&credential.scheme).map_err(|_| refuse("unsupported scheme"))?;
            let verified = webauthn::verify(
                scheme,
                &webauthn::Registered {
                    cose_key: &credential.cose_key,
                    rp_id: &config.rp_id,
                    origin: &config.origin,
                    recorded_sign_count: count,
                },
                assertion,
                &challenge.message()?,
            )
            .map_err(|reason| refuse(&reason))?;
            let receipt = Receipt {
                decision: format!("urn:quipu:decision:{nonce}"),
                challenge,
                cose_key: credential.cose_key,
                scheme: credential.scheme,
                authenticator_data: assertion.authenticator_data.to_vec(),
                client_data_json: assertion.client_data_json.to_vec(),
                signature: assertion.signature.to_vec(),
                recorded_at: time,
            };
            // Unique(request,revision) is semantic idempotence: another fresh
            // nonce cannot turn the same logical decision into a second action.
            store.conn.execute(
                "INSERT INTO deck_receipts(nonce,request,revision,data) VALUES(?1,?2,?3,?4)",
                params![
                    nonce,
                    receipt.challenge.request.id,
                    receipt.challenge.request.revision,
                    encode(&receipt)?
                ],
            )?;
            if store.conn.execute(
                "UPDATE deck_challenges SET consumed=1 WHERE nonce=?1 AND consumed=0",
                [nonce],
            )? != 1
            {
                return Err(refuse("replayed challenge"));
            }
            store.conn.execute(
                "UPDATE deck_credentials SET sign_count=?2 WHERE id=?1",
                params![receipt.challenge.credential, verified.sign_count],
            )?;
            let mut datums = vec![typed(store, &receipt.decision, "Decision", ts)?];
            for (field, value) in [
                ("outcome", Value::Str(receipt.challenge.choice.clone())),
                ("by", Value::Str(receipt.challenge.actor.clone())),
                (
                    "evidenceHash",
                    Value::Str(crate::share::sha256(&receipt.challenge.message()?)),
                ),
                (
                    "forRequest",
                    Value::Ref(store.intern(&receipt.challenge.request.id)?),
                ),
                (
                    "deckCredential",
                    Value::Str(receipt.challenge.credential.clone()),
                ),
                ("deckRecordedAt", Value::Int(receipt.recorded_at)),
                ("deckScheme", Value::Str(receipt.scheme.clone())),
                ("deckAdmission", Value::Str("verified-at-commit".into())),
                (
                    "deckReceipt",
                    Value::Str(
                        String::from_utf8(encode(&receipt)?)
                            .map_err(|_| refuse("receipt encoding"))?,
                    ),
                ),
            ] {
                datums.push(datum(store, &receipt.decision, field, value, ts)?);
            }
            Ok((receipt, datums))
        })
    }

    /// Read-only exact receipt lookup, including while disabled or revoked.
    /// Never retry a decision automatically after an indeterminate response.
    pub fn deck_receipt(&self, nonce: &str) -> Result<Option<(Receipt, i64)>> {
        let row: Option<(Vec<u8>, i64)> = self
            .conn
            .query_row(
                "SELECT data,tx FROM deck_receipts WHERE nonce=?1",
                [nonce],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        row.map(|(data, tx)| Ok((decode(&data)?, tx))).transpose()
    }

    /// Historical re-verification of a PROTECTED admitted receipt, with its
    /// retained enrollment and server witness. Later revocation prevents future
    /// admission but does not erase history. This grants no execution authority.
    pub fn deck_verify_receipt(&self, nonce: &str) -> Result<bool> {
        let Some((receipt, _)) = self.deck_receipt(nonce)? else {
            return Ok(false);
        };
        if receipt.recorded_at >= receipt.challenge.expires_at {
            return Ok(false);
        }
        let scheme =
            Scheme::parse(&receipt.scheme).map_err(|_| refuse("unsupported audit scheme"))?;
        Ok(webauthn::verify(
            scheme,
            &webauthn::Registered {
                cose_key: &receipt.cose_key,
                rp_id: &receipt.challenge.rp_id,
                origin: &receipt.challenge.origin,
                recorded_sign_count: 0,
            },
            &webauthn::Assertion {
                authenticator_data: &receipt.authenticator_data,
                client_data_json: &receipt.client_data_json,
                signature: &receipt.signature,
            },
            &receipt.challenge.message()?,
        )
        .is_ok())
    }
}
