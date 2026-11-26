//! The read-side provenance object (aegis-7tt P3): what `/transactions` and
//! `/entity_history` report about who and where a committed write came from.
//!
//! It is computed ON READ from the stored declaration row and the transaction's
//! authentication row, never stored, and its completeness class comes from
//! [`read_class`]: the SAME [`RequestProvenance::classify`] and
//! [`RequestProvenance::with_verified_identity`] the write metric counts with,
//! so the read and the metric cannot disagree about one request.
//!
//! Precedence: on an `attested_session` transaction the agent is the verified
//! principal and the session is the binding session (P2). A declared agent or
//! session on that transaction is reported under `superseded`, never as
//! `effective`. Harness, host and model are always declared.
//!
//! `state` says whether there was anything to read: `observed` (a declaration
//! row), `unknown-no-row` (the table exists but this transaction wrote no row,
//! for example an import or a write from before P1), or `unknown-no-table` (a
//! read-only open of a store that predates the table). An unknown state is
//! never an empty declaration: its completeness is `unknown`.

use std::collections::BTreeMap;

use rusqlite::OptionalExtension;
use serde_json::{Map, Value, json};

use crate::write_provenance::{FIELDS, RequestProvenance};

/// Whether a declaration row was there to read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadState {
    Observed,
    UnknownNoRow,
    UnknownNoTable,
}

impl ReadState {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Observed => "observed",
            Self::UnknownNoRow => "unknown-no-row",
            Self::UnknownNoTable => "unknown-no-table",
        }
    }
}

/// The class the write metric counted for a request with these stored
/// declarations: `classify`, then `with_verified_identity` when the
/// transaction's attestation verified. The one helper both sides share.
#[must_use]
pub fn read_class(declarations: &BTreeMap<String, String>, verified: bool) -> RequestProvenance {
    let values = FIELDS.map(|f| declarations.get(f).map(String::as_str));
    let classified = RequestProvenance::classify("", "", values);
    if verified {
        classified.with_verified_identity()
    } else {
        classified
    }
}

fn entry(value: &str, basis: &str) -> Value {
    json!({ "value": value, "basis": basis })
}

/// Assemble the object from what was read. `principal` is `Some` only for an
/// `attested_session` transaction; `binding_session` is its persisted session.
#[must_use]
pub fn provenance_object(
    declarations: Option<&BTreeMap<String, String>>,
    state: ReadState,
    principal: Option<&str>,
    binding_session: Option<&str>,
) -> Value {
    let mut effective = Map::new();
    let mut superseded = Map::new();
    if let Some(principal) = principal {
        effective.insert("agent".into(), entry(principal, "attested"));
        if let Some(session) = binding_session {
            effective.insert("session".into(), entry(session, "attested"));
        }
    }
    for field in FIELDS {
        let Some(value) = declarations.and_then(|d| d.get(field)) else {
            continue;
        };
        let identity = matches!(field, "agent" | "session");
        let slot = if identity && principal.is_some() {
            &mut superseded
        } else {
            &mut effective
        };
        slot.insert(field.into(), entry(value, "declared"));
    }
    let completeness = declarations.map_or("unknown", |d| {
        read_class(d, principal.is_some()).completeness.as_str()
    });
    disclose(json!({
        "effective": effective,
        "superseded": superseded,
        "completeness": completeness,
        "state": state.as_str(),
    }))
}

/// The single seam for the P4 grant split, which will redact host and session
/// for a reader without the grant. Today every reader sees the whole object.
fn disclose(provenance: Value) -> Value {
    provenance
}

impl crate::Store {
    /// The stored declaration row of `tx`, with whether there was one to read.
    fn declaration_row(
        &self,
        tx: i64,
    ) -> crate::Result<(Option<BTreeMap<String, String>>, ReadState)> {
        let exists: bool = self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM main.sqlite_master \
             WHERE type='table' AND name='transaction_declarations')",
            [],
            |row| row.get(0),
        )?;
        if !exists {
            return Ok((None, ReadState::UnknownNoTable));
        }
        let text: Option<String> = self
            .conn
            .query_row(
                "SELECT declarations FROM main.transaction_declarations WHERE tx=?1",
                [tx],
                |row| row.get(0),
            )
            .optional()?;
        let Some(text) = text else {
            return Ok((None, ReadState::UnknownNoRow));
        };
        let map =
            serde_json::from_str(&text).map_err(|e| crate::Error::Serialization(e.to_string()))?;
        Ok((Some(map), ReadState::Observed))
    }

    /// The read-side provenance object of transaction `tx`.
    pub fn transaction_provenance(&self, tx: i64) -> crate::Result<Value> {
        let (declarations, state) = self.declaration_row(tx)?;
        let principal = self
            .transaction_auth(tx)?
            .filter(|a| a.auth_class == "attested_session")
            .map(|a| a.principal);
        let binding_session = match principal {
            Some(_) => self.transaction_attested_session(tx)?,
            None => None,
        };
        Ok(provenance_object(
            declarations.as_ref(),
            state,
            principal.as_deref(),
            binding_session.as_deref(),
        ))
    }
}

#[cfg(test)]
#[path = "write_provenance_read_tests.rs"]
mod tests;
