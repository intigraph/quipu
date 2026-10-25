//! Operator-only native configuration, over the store file. No transport
//! caller enrolls a signer, protects a graph or flips the switch.

use super::*;

impl Store {
    pub(in crate::store) fn migrate_attested_write(conn: &rusqlite::Connection) -> Result<()> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS attest_switch (
               id INTEGER PRIMARY KEY CHECK(id=1),
               enabled INTEGER NOT NULL DEFAULT 0 CHECK(enabled IN (0,1)));
             INSERT OR IGNORE INTO attest_switch(id,enabled) VALUES(1,0);
             CREATE TABLE IF NOT EXISTS attest_signers (
               id TEXT PRIMARY KEY, data BLOB NOT NULL,
               revoked INTEGER NOT NULL DEFAULT 0, sign_count INTEGER NOT NULL DEFAULT 0);
             CREATE TABLE IF NOT EXISTS attest_graphs (iri TEXT PRIMARY KEY);
             CREATE TABLE IF NOT EXISTS attest_challenges (
               nonce TEXT PRIMARY KEY, data BLOB NOT NULL, consumed INTEGER NOT NULL DEFAULT 0);
             CREATE TABLE IF NOT EXISTS attest_receipts (
               nonce TEXT PRIMARY KEY, data BLOB NOT NULL, tx INTEGER);",
        )?;
        Ok(())
    }

    /// The operator switch. Off by default; off refuses every challenge and
    /// commit, while receipts stay readable.
    pub fn attest_set_enabled(&mut self, enabled: bool) -> Result<()> {
        self.conn
            .execute("UPDATE attest_switch SET enabled=?1 WHERE id=1", [enabled])?;
        Ok(())
    }

    /// Read the local operator switch without changing it.
    pub fn attest_enabled(&self) -> Result<bool> {
        Ok(self
            .conn
            .query_row("SELECT enabled FROM attest_switch WHERE id=1", [], |r| {
                r.get(0)
            })
            .optional()?
            .unwrap_or(false))
    }

    /// Mark a named graph as accepting attested writes only, registering it
    /// if needed so that a precondition can read it before its first write.
    /// Ordinary writes to it are refused from then on. Irreversible by design.
    pub fn attest_protect_graph(&mut self, graph: &str) -> Result<()> {
        if !iri(graph)
            || graph.starts_with("urn:quipu:")
            || super::super::inferred::is_inferred_graph_iri(graph)
        {
            return Err(refuse("invalid or reserved graph IRI"));
        }
        self.graph_create(graph)?;
        self.conn.execute(
            "INSERT OR IGNORE INTO attest_graphs(iri) VALUES(?1)",
            [graph],
        )?;
        Ok(())
    }

    /// Whether `graph` accepts attested writes only.
    pub fn attest_graph_is_protected(&self, graph: &str) -> Result<bool> {
        Ok(self
            .conn
            .query_row("SELECT 1 FROM attest_graphs WHERE iri=?1", [graph], |_| {
                Ok(())
            })
            .optional()?
            .is_some())
    }

    /// Enroll a signer after an independent operator ceremony. A retained ID
    /// is never replaced or resurrected; rotation enrolls a new ID.
    pub fn attest_enroll_signer(&mut self, signer: &Signer) -> Result<()> {
        use base64::Engine as _;
        let id = crate::verdict_schemes::b64url_decode(&signer.id)
            .map_err(|_| refuse("signer ID encoding"))?;
        let host_ok = signer.rp_id.contains('.')
            && signer
                .rp_id
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'.' || b == b'-')
            && !signer
                .rp_id
                .split('.')
                .any(|s| s.is_empty() || s.starts_with('-') || s.ends_with('-'));
        if id.is_empty()
            || id.len() > 1024
            || base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&id) != signer.id
            || signer.expires_at <= now()?
            || signer.origin != format!("https://{}", signer.rp_id)
            || !host_ok
            || signer.actor.is_empty()
            || signer.actor.len() > MAX_TEXT_BYTES
            || signer.graphs.is_empty()
            || signer.graphs.len() > 16
            || !signer.graphs.iter().all(|g| iri(g))
        {
            return Err(refuse(
                "invalid signer ID, expiry, origin/RP, identity or graph list",
            ));
        }
        let key = crate::verdict_schemes::cose::parse(&signer.cose_key)
            .map_err(|_| refuse("invalid COSE key"))?;
        match (
            Scheme::parse(&signer.scheme).map_err(|_| refuse("unsupported scheme"))?,
            key,
        ) {
            (Scheme::WebauthnEs256, crate::verdict_schemes::cose::CoseKey::Es256 { .. })
            | (Scheme::WebauthnEddsa, crate::verdict_schemes::cose::CoseKey::Ed25519 { .. }) => {}
            _ => return Err(refuse("scheme/key mismatch")),
        }
        self.conn.execute(
            "INSERT INTO attest_signers(id,data) VALUES(?1,?2)",
            params![signer.id, encode(signer)?],
        )?;
        Ok(())
    }

    /// Revoke permanently. Receipts stay verifiable.
    pub fn attest_revoke_signer(&mut self, id: &str) -> Result<bool> {
        Ok(self
            .conn
            .execute("UPDATE attest_signers SET revoked=1 WHERE id=?1", [id])?
            == 1)
    }

    /// Every enrolled signer, oldest first, including revoked and expired.
    pub fn attest_signers(&self) -> Result<Vec<SignerStatus>> {
        let mut statement = self
            .conn
            .prepare("SELECT data,revoked,sign_count FROM attest_signers ORDER BY rowid")?;
        let rows = statement
            .query_map([], |r| Ok((r.get::<_, Vec<u8>>(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(|(data, revoked, sign_count)| {
                let signer: Signer = decode(&data)?;
                Ok(SignerStatus {
                    fingerprint: key_fingerprint(&signer.cose_key),
                    signer,
                    revoked,
                    sign_count,
                })
            })
            .collect()
    }

    /// An active (unrevoked, unexpired) signer and its counter.
    pub(super) fn attest_active_signer(&self, id: &str) -> Result<(Signer, u32)> {
        let row: Option<(Vec<u8>, bool, u32)> = self
            .conn
            .query_row(
                "SELECT data,revoked,sign_count FROM attest_signers WHERE id=?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        let (data, revoked, count) = row.ok_or_else(|| refuse("unknown signer"))?;
        let signer: Signer = decode(&data)?;
        if revoked || signer.expires_at <= now()? {
            return Err(refuse("revoked or expired signer"));
        }
        Ok((signer, count))
    }
}
