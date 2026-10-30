pub use crate::store_replica::{Cell, Column, Table};
use rusqlite::types::{Value, ValueRef};

use super::invalid;
use crate::{Result, Store};

// Query data, including history and source transaction ids. No source-local
// owner, credential, nonce, signing key, delivery cursor or in-flight upload.
const DATA: &[&str] = &[
    "terms",
    "transactions",
    "facts",
    "graphs",
    "shapes",
    "ontologies",
    "schema_terms",
    "queries",
    "query_params",
    "datasets",
    "dataset_members",
    "forks",
    "graph_events",
    "pendant_repo_graphs",
    "pendant_repo_pointers",
    "pendant_repo_tombstones",
    "term_spaces",
    "proposals",
    "proposal_migrations",
    "import_reviews",
    "events",
    "pending_share_queries",
    "denial_quarantine",
    "quarantine_deltas",
    "vectors",
    "search_default_policy",
];
const LOCAL: &[&str] = &[
    // Authentication adjudication is local evidence, not transferable writer
    // authority. Actor/source text in historical transactions remains data.
    "transaction_auth",
    "store_identity",
    "log_epoch",
    "consumers",
    "subscriptions",
    "graph_consumers",
    "snapshot_uploads",
    "snapshot_upload_parts",
    "attestation_bindings",
    "attestation_nonces",
    "ownership_enforcement",
    "frozen_packs",
    "store_replication_state",
    "store_replication_changes",
    "replica_progress",
    "replica_applied_tx",
    "replica_stop",
    "replica_coverage",
    "replica_tx_map",
    "replica_source_end",
    "replica_bootstrap_admission",
    "follow_caught_up",
    // Operator enrollment, write grants, nonces and live ceremonies stay
    // local. A follower gets no authority by copying primary credentials.
    "attest_challenges",
    "attest_graphs",
    "attest_receipts",
    "attest_signers",
    "attest_switch",
    "decision_nonces",
    "deck_challenges",
    "deck_config",
    "deck_credentials",
    "deck_receipts",
    "deck_requests",
    "registry_amendment_nonces",
];

impl Cell {
    pub(crate) fn from_sql(value: ValueRef<'_>) -> Result<Self> {
        let (kind, value) = match value {
            ValueRef::Null => ("null", serde_json::Value::Null),
            ValueRef::Integer(n) => ("integer", n.into()),
            ValueRef::Real(n) if n.is_finite() => ("real", n.to_string().into()),
            ValueRef::Text(bytes) => (
                "text",
                std::str::from_utf8(bytes)
                    .map_err(|_| invalid("logical store contains non-UTF8 text"))?
                    .into(),
            ),
            ValueRef::Blob(bytes) => ("blob", hex::encode_upper(bytes).into()),
            ValueRef::Real(_) => {
                return Err(invalid("logical store contains unsupported real value"));
            }
        };
        Ok(Self {
            kind: kind.into(),
            value,
        })
    }

    pub(crate) fn to_sql(&self) -> Result<Value> {
        let bad = || invalid("invalid logical store cell");
        Ok(match self.kind.as_str() {
            "null" if self.value.is_null() => Value::Null,
            "integer" => Value::Integer(self.value.as_i64().ok_or_else(bad)?),
            "text" => Value::Text(self.value.as_str().ok_or_else(bad)?.into()),
            "blob" => {
                Value::Blob(hex::decode(self.value.as_str().ok_or_else(bad)?).map_err(|_| bad())?)
            }
            "real" => {
                let n: f64 = self
                    .value
                    .as_str()
                    .ok_or_else(bad)?
                    .parse()
                    .map_err(|_| bad())?;
                if !n.is_finite() {
                    return Err(bad());
                }
                Value::Real(n)
            }
            _ => return Err(bad()),
        })
    }
}

pub(crate) fn quote(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

pub(crate) fn inspect(store: &Store) -> Result<Vec<Table>> {
    if !store.attachments().is_empty() {
        return Err(invalid(
            "logical full-store cut requires admitted attachment closure",
        ));
    }
    let frozen: bool = store
        .conn
        .prepare("SELECT 1 FROM frozen_packs WHERE thawed_at IS NULL LIMIT 1")?
        .exists([])?;
    if frozen {
        return Err(invalid(
            "logical full-store cut requires admitted frozen-pack closure",
        ));
    }
    let names: Vec<String> = store.conn.prepare(
        "SELECT name FROM main.sqlite_schema WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name")?
        .query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
    let unknown: Vec<_> = names
        .iter()
        .filter(|n| !DATA.contains(&n.as_str()) && !LOCAL.contains(&n.as_str()))
        .collect();
    if !unknown.is_empty() {
        return Err(invalid(format!(
            "unclassified logical store tables: {unknown:?}"
        )));
    }
    let mut catalog = Vec::new();
    for name in names.into_iter().filter(|n| DATA.contains(&n.as_str())) {
        let columns = store
            .conn
            .prepare(&format!("PRAGMA main.table_info({})", quote(&name)))?
            .query_map([], |r| {
                Ok(Column {
                    name: r.get(1)?,
                    declared_type: r.get(2)?,
                    not_null: r.get::<_, i64>(3)? != 0,
                    primary_key: r.get(5)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let create_sql = store.conn.query_row(
            "SELECT sql FROM main.sqlite_schema WHERE type='table' AND name=?1",
            [&name],
            |r| r.get(0),
        )?;
        let table = Table {
            name,
            columns,
            create_sql,
        };
        if table.key_indices().is_empty() {
            return Err(invalid(format!(
                "logical store table has no primary key: {}",
                table.name
            )));
        }
        catalog.push(table);
    }
    Ok(catalog)
}

pub(crate) fn digest(tables: &[Table]) -> Result<String> {
    use sha2::{Digest, Sha256};
    Ok(hex::encode(Sha256::digest(
        serde_json::to_vec(tables).map_err(|error| super::json_error(&error))?,
    )))
}
