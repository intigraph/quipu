//! Challenge issuance, commit and read-only receipt recovery.

use ring::rand::{SecureRandom as _, SystemRandom};

use super::*;
use crate::namespace::{DEFAULT_BASE_NS, RDF_TYPE};

/// The receipt node for one admitted write.
pub fn receipt_iri(nonce: &str) -> String {
    format!("urn:quipu:attest:{nonce}")
}

/// Terms only the server may write: the receipt nodes and the provenance
/// vocabulary. A proposal that uses them is refused, so no signed or ordinary
/// triple can look like a server-authored attestation.
fn reserved(term: &str) -> bool {
    term.starts_with("urn:quipu:")
        || term
            .strip_prefix(DEFAULT_BASE_NS)
            .is_some_and(|local| local.starts_with("attest") || local == "AttestedWrite")
}

fn valid_proposal(proposal: &Proposal) -> bool {
    !proposal.datums.is_empty()
        && !proposal.datums.iter().any(|d| {
            reserved(&d.s) || reserved(&d.p) || matches!(&d.o, TypedValue::Iri(i) if reserved(i))
        })
        && proposal.datums.len() <= MAX_DATUMS
        && iri(&proposal.graph)
        && proposal
            .datums
            .iter()
            .all(|d| iri(&d.s) && iri(&d.p) && valid_value(&d.o))
        && expect::valid_expect(&proposal.expect)
}

impl Store {
    /// The signer is active, the graph is attested-only and on its list.
    fn attest_scope(&self, proposal: &Proposal) -> Result<Signer> {
        let (signer, _) = self.attest_active_signer(&proposal.signer)?;
        if !signer.graphs.contains(&proposal.graph)
            || !self.attest_graph_is_protected(&proposal.graph)?
        {
            return Err(refuse(
                "graph is not attested-only or not on the signer's list",
            ));
        }
        Ok(signer)
    }

    /// Store and return one expiring challenge over the exact proposal.
    /// Preconditions are checked now too, so a stale proposal is refused
    /// before anyone is asked to sign.
    pub fn attest_challenge(&mut self, proposal: &Proposal) -> Result<Challenge> {
        if !valid_proposal(proposal) {
            return Err(refuse("invalid proposal"));
        }
        self.attest_atomic(|store, _| {
            if !store.attest_enabled()? {
                return Err(refuse("disabled"));
            }
            let signer = store.attest_scope(proposal)?;
            store.attest_check_expect(&proposal.expect)?;
            let mut bytes = [0u8; 32];
            SystemRandom::new()
                .fill(&mut bytes)
                .map_err(|_| refuse("nonce entropy unavailable"))?;
            let challenge = Challenge {
                domain: DOMAIN.into(),
                audience: store.store_id()?,
                nonce: hex::encode(bytes),
                origin: signer.origin,
                rp_id: signer.rp_id,
                actor: signer.actor,
                proposal: proposal.clone(),
                expires_at: now()?
                    .checked_add(CHALLENGE_SECS)
                    .ok_or_else(|| refuse("clock overflow"))?,
            };
            if challenge.message()?.len() > MAX_MESSAGE_BYTES {
                return Err(refuse("proposal too large"));
            }
            store.conn.execute(
                "INSERT INTO attest_challenges(nonce,data) VALUES(?1,?2)",
                params![challenge.nonce, encode(&challenge)?],
            )?;
            Ok((challenge, None))
        })
    }

    /// Verify the stored challenge's assertion and, in one savepoint, check
    /// the preconditions, consume the challenge, advance the counter, write
    /// the datums and record the receipt. A second commit is refused even if
    /// the first response was lost; recover with [`Store::attest_receipt`].
    pub fn attest_commit(
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
        self.attest_atomic(|store, ts| {
            if !store.attest_enabled()? {
                return Err(refuse("disabled"));
            }
            let row: Option<(Vec<u8>, bool)> = store
                .conn
                .query_row(
                    "SELECT data,consumed FROM attest_challenges WHERE nonce=?1",
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
            let signer = store.attest_scope(&challenge.proposal)?;
            if challenge.expires_at <= time
                || challenge.audience != store.store_id()?
                || challenge.origin != signer.origin
                || challenge.rp_id != signer.rp_id
                || challenge.actor != signer.actor
            {
                return Err(refuse("expired challenge or scope mismatch"));
            }
            store.attest_check_expect(&challenge.proposal.expect)?;
            let (_, count) = store.attest_active_signer(&signer.id)?;
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
                || client.origin != signer.origin
                || client.challenge.is_empty()
            {
                return Err(refuse("invalid client data or cross-origin assertion"));
            }
            let scheme = Scheme::parse(&signer.scheme).map_err(|_| refuse("unsupported scheme"))?;
            let message = challenge.message()?;
            let verified = webauthn::verify(
                scheme,
                &webauthn::Registered {
                    cose_key: &signer.cose_key,
                    rp_id: &signer.rp_id,
                    origin: &signer.origin,
                    recorded_sign_count: count,
                },
                assertion,
                &message,
            )
            .map_err(|reason| refuse(&reason))?;
            let receipt = Receipt {
                content_hash: crate::share::sha256(&message),
                challenge,
                cose_key: signer.cose_key.clone(),
                scheme: signer.scheme.clone(),
                authenticator_data: assertion.authenticator_data.to_vec(),
                client_data_json: assertion.client_data_json.to_vec(),
                signature: assertion.signature.to_vec(),
                recorded_at: time,
            };
            store.conn.execute(
                "INSERT INTO attest_receipts(nonce,data) VALUES(?1,?2)",
                params![nonce, encode(&receipt)?],
            )?;
            if store.conn.execute(
                "UPDATE attest_challenges SET consumed=1 WHERE nonce=?1 AND consumed=0",
                [nonce],
            )? != 1
            {
                return Err(refuse("replayed challenge"));
            }
            store.conn.execute(
                "UPDATE attest_signers SET sign_count=?2 WHERE id=?1",
                params![signer.id, verified.sign_count],
            )?;
            let datums = store.attest_datums(&receipt, nonce, ts)?;
            let graph = receipt.challenge.proposal.graph.clone();
            Ok((receipt, Some((graph, signer.actor, datums))))
        })
    }

    /// The proposal's datums, plus domain-free provenance: each written
    /// subject `attestedBy` the receipt node, which names the signer, actor,
    /// content hash, server time and the exact preconditions.
    fn attest_datums(&self, receipt: &Receipt, nonce: &str, ts: &str) -> Result<Vec<Datum>> {
        let assert = |entity: i64, attribute: i64, value: Value| Datum {
            entity,
            attribute,
            value,
            valid_from: ts.into(),
            valid_to: None,
            op: Op::Assert,
        };
        let term = |name: &str| self.intern(&format!("{DEFAULT_BASE_NS}{name}"));
        let node = self.intern(&receipt_iri(nonce))?;
        let mut datums = Vec::new();
        let mut subjects = std::collections::BTreeSet::new();
        for d in &receipt.challenge.proposal.datums {
            let value = match &d.o {
                TypedValue::Int(i) => Value::Int(*i),
                TypedValue::Str(s) => Value::Str(s.clone()),
                TypedValue::Iri(i) => Value::Ref(self.intern(i)?),
            };
            let s = self.intern(&d.s)?;
            subjects.insert(s);
            datums.push(assert(s, self.intern(&d.p)?, value));
        }
        let by = term("attestedBy")?;
        for s in subjects {
            datums.push(assert(s, by, Value::Ref(node)));
        }
        datums.push(assert(
            node,
            self.intern(RDF_TYPE)?,
            Value::Ref(term("AttestedWrite")?),
        ));
        let challenge = &receipt.challenge;
        for (field, value) in [
            (
                "attestSigner",
                Value::Str(challenge.proposal.signer.clone()),
            ),
            ("attestActor", Value::Str(challenge.actor.clone())),
            (
                "attestContentHash",
                Value::Str(receipt.content_hash.clone()),
            ),
            ("attestRecordedAt", Value::Int(receipt.recorded_at)),
            ("attestGraph", Value::Str(challenge.proposal.graph.clone())),
            (
                "attestProposal",
                Value::Str(
                    String::from_utf8(encode(&challenge.proposal)?)
                        .map_err(|_| refuse("proposal encoding"))?,
                ),
            ),
            (
                "attestExpect",
                Value::Str(
                    String::from_utf8(encode(&challenge.proposal.expect)?)
                        .map_err(|_| refuse("expectation encoding"))?,
                ),
            ),
        ] {
            datums.push(assert(node, term(field)?, value));
        }
        Ok(datums)
    }

    /// Read-only exact receipt lookup, also while disabled or revoked.
    pub fn attest_receipt(&self, nonce: &str) -> Result<Option<(Receipt, Option<i64>)>> {
        let row: Option<(Vec<u8>, Option<i64>)> = self
            .conn
            .query_row(
                "SELECT data,tx FROM attest_receipts WHERE nonce=?1",
                [nonce],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        row.map(|(data, tx)| Ok((decode(&data)?, tx))).transpose()
    }

    /// Re-verify a retained receipt against its key snapshot. Later
    /// revocation stops future writes; it does not erase history.
    pub fn attest_verify_receipt(&self, nonce: &str) -> Result<bool> {
        let Some((receipt, _)) = self.attest_receipt(nonce)? else {
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
