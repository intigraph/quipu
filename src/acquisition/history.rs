//! Retained MAIN history, not a composed historical snapshot or global clock.

use rusqlite::{OptionalExtension, params};
use serde::Deserialize;
use serde_json::{Value as JsonValue, json};

use super::{invalid, main_graph};
use crate::{Result, Store, Value};

/// Consumers must recognize this contract before interpreting its receipts.
pub const CONTRACT: &str = "quipu-entity-history/main-v1";

/// Metadata observation used to bootstrap a large subject's bounded ranges.
pub const OBSERVATION_CONTRACT: &str = "quipu-entity-history/observation-v1";

/// One complete bounded range for one exact subject in one MAIN graph.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    /// Exact absolute subject IRI; no interning or fuzzy lookup.
    pub iri: String,
    /// Registered committed MAIN graph, never a dataset or provider alias.
    pub graph: String,
    /// Expected persistent MAIN lineage.
    pub source_store_id: String,
    /// Required with explicit transaction bounds; changes on reopen/remount.
    pub expected_generation: Option<String>,
    /// Exclusive lower bound in this source's local transaction namespace.
    pub since_tx: Option<i64>,
    /// Inclusive upper bound, not a historical policy or composition pin.
    pub until_tx: Option<i64>,
    /// Lowerable row cap, default 1000 and maximum 10000. Overflow refuses.
    pub max_entries: Option<usize>,
    /// Lowerable response byte cap, default 1 MiB and maximum 4 MiB.
    pub max_bytes: Option<usize>,
    /// Observe source generation and bounds without returning any history.
    #[serde(default)]
    pub observation_only: bool,
}

struct Budget<'a> {
    conn: &'a rusqlite::Connection,
    deadline: crate::time::Deadline,
    request: Option<crate::time::Deadline>,
}

impl<'a> Budget<'a> {
    fn install(store: &'a Store) -> Result<Self> {
        let deadline = crate::time::Deadline::after_millis(1000);
        let request = crate::time::request_deadline();
        store.conn.progress_handler(
            1000,
            Some(move || deadline.passed() || request.is_some_and(|d| d.passed())),
        )?;
        Ok(Self {
            conn: &store.conn,
            deadline,
            request,
        })
    }

    fn check(&self) -> Result<()> {
        if self.deadline.passed() || self.request.is_some_and(|d| d.passed()) {
            return Err(invalid("history deadline exceeded; completeness UNKNOWN"));
        }
        Ok(())
    }
}

impl Drop for Budget<'_> {
    fn drop(&mut self) {
        let _ = self.conn.progress_handler(0, None::<fn() -> bool>);
    }
}

fn value(store: &Store, value: Value) -> Result<JsonValue> {
    Ok(match value {
        Value::Ref(id) => json!({"type":"iri", "value":main_graph::term(store, id)?}),
        Value::Str(text) => json!({"type":"string", "value":text}),
        Value::Int(n) => json!({"type":"integer", "value":n.to_string()}),
        Value::Float(n) => json!({"type":"float64", "bits":format!("{:016x}", n.to_bits())}),
        Value::Bool(b) => json!({"type":"boolean", "value":b}),
        Value::Bytes(bytes) => {
            json!({"type":"bytes", "encoding":"hex", "value":hex::encode(bytes)})
        }
        Value::Lang { lexical, lang } => json!({"type":"lang", "value":lexical, "lang":lang}),
        Value::Typed { lexical, datatype } => {
            json!({"type":"typed", "value":lexical, "datatype":datatype})
        }
    })
}

/// Read retained physical MAIN facts under one current policy and SQLite cut.
///
/// A complete response certifies only this bounded subject/graph/range in MAIN.
/// Attachments and earlier provider generations may contain additional history;
/// continuous history and historical policy eligibility are explicitly unknown.
pub fn read(store: &Store, request: &Request) -> Result<JsonValue> {
    let rows_cap = request.max_entries.unwrap_or(1000);
    let bytes_cap = request.max_bytes.unwrap_or(1024 * 1024);
    if !(1..=10000).contains(&rows_cap) || !(1..=4 * 1024 * 1024).contains(&bytes_cap) {
        return Err(invalid("history row or byte cap outside supported bounds"));
    }
    if request.iri.len() > 8192 || request.graph == "urn:quipu:graph:root" {
        return Err(invalid(
            "qualified history requires an exact named graph and bounded IRI",
        ));
    }
    oxrdf::NamedNode::new(&request.iri).map_err(|e| invalid(e.to_string()))?;
    oxrdf::NamedNode::new(&request.graph).map_err(|e| invalid(e.to_string()))?;
    if (request.since_tx.is_some() || request.until_tx.is_some())
        && request.expected_generation.is_none()
    {
        return Err(invalid(
            "history bounds require their observed source generation",
        ));
    }
    if request.observation_only && (request.since_tx.is_some() || request.until_tx.is_some()) {
        return Err(invalid(
            "history observation is not a transaction range or pin",
        ));
    }
    let budget = Budget::install(store)?;
    budget.check()?;
    let snapshot = store.conn.unchecked_transaction()?;
    let source_id = store.store_id()?;
    if source_id != request.source_store_id || store.acquisition_incarnation.is_empty() {
        return Err(invalid(
            "history source identity mismatch or unknown incarnation",
        ));
    }
    let (floor, head): (Option<i64>, Option<i64>) =
        store
            .conn
            .query_row("SELECT MIN(id), MAX(id) FROM main.transactions", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })?;
    let (Some(floor), Some(head)) = (floor, head) else {
        return Err(invalid("history source has no committed transaction"));
    };
    if floor <= 0 || head < floor {
        return Err(invalid("history source transaction bounds are invalid"));
    }
    let (graph, metadata) = main_graph::metadata(store, &request.graph)?;
    let aliases: Vec<_> = store.attachments.iter().map(|a| a.alias.as_str()).collect();
    let labels = store.labels_config();
    let generation = super::composed::digest(&json!({
        "schema":CONTRACT, "source_store_id":source_id,
        "incarnation":store.acquisition_incarnation, "graph_metadata":metadata,
        "retained_log_min_tx":floor, "excluded_attachment_aliases":aliases,
        "label_floor":{
            "freshness":labels.min_freshness, "trust":labels.min_trust_rank,
            "chain":labels.min_trust_chain, "policy":labels.deny_policy_tokens,
            "kind":labels.deny_data_kinds,
        },
    }))?;
    if request
        .expected_generation
        .as_ref()
        .is_some_and(|g| g != &generation)
    {
        return Err(invalid(
            "history generation mismatch; reopen, topology or policy changed",
        ));
    }
    let since = request.since_tx.unwrap_or(floor - 1);
    let until = request.until_tx.unwrap_or(head);
    if since < floor - 1 || since > until || until > head || until < floor {
        return Err(invalid("history range is outside retained source bounds"));
    }
    for tx in [request.since_tx, request.until_tx].into_iter().flatten() {
        if tx != floor - 1
            && !store
                .conn
                .prepare("SELECT 1 FROM main.transactions WHERE id=?1")?
                .exists([tx])?
        {
            return Err(invalid(
                "history bound is not a committed local transaction",
            ));
        }
    }
    let entity: i64 = store
        .conn
        .query_row(
            "SELECT id FROM main.terms WHERE iri=?1",
            [&request.iri],
            |r| r.get(0),
        )
        .optional()?
        .ok_or_else(|| invalid("history subject is not owned by main"))?;
    if main_graph::term(store, entity)? != request.iri {
        return Err(invalid("history subject identity mismatch"));
    }
    if request.observation_only {
        let observation = json!({
            "schema":OBSERVATION_CONTRACT,"authority":"observation-only; not history or historical-policy proof",
            "iri":request.iri,"graph":request.graph,"source_store_id":source_id,
            "generation":generation,"incarnation":store.acquisition_incarnation,
            "observed_head_tx":head,"retained_log_min_tx":floor,
            "excluded_attachment_aliases":aliases,
        });
        if serde_json::to_vec(&observation)
            .map_err(|e| invalid(e.to_string()))?
            .len()
            > bytes_cap
        {
            return Err(invalid("history observation byte cap exceeded"));
        }
        budget.check()?;
        snapshot.commit()?;
        return Ok(observation);
    }
    let history = {
        let mut q = store.conn.prepare(
            "SELECT a,CASE WHEN length(v)<=?5 THEN v ELSE NULL END,tx,valid_from,valid_to,op \
             FROM main.facts WHERE e=?1 AND g=?2 AND tx>?3 AND tx<=?4 ORDER BY tx,a,rowid LIMIT ?6",
        )?;
        let mut rows = q.query(params![
            entity,
            graph,
            since,
            until,
            bytes_cap as i64,
            (rows_cap + 1) as i64
        ])?;
        let mut entries = Vec::new();
        let mut bytes = 0;
        while let Some(row) = rows.next()? {
            budget.check()?;
            if entries.len() == rows_cap {
                return Err(invalid("history row cap exceeded; completeness UNKNOWN"));
            }
            let blob: Vec<u8> = row
                .get::<_, Option<Vec<u8>>>(1)?
                .ok_or_else(|| invalid("history value byte cap exceeded"))?;
            let tx: i64 = row.get(2)?;
            let entry = json!({
                "predicate":main_graph::term(store,row.get(0)?)?,
                "value":value(store,Value::from_bytes(&blob)?)?,
                "op":match row.get::<_,i32>(5)? { 1=>"assert", 0=>"retract", _=>return Err(invalid("unknown history operation")) },
                "valid_from":row.get::<_,String>(3)?, "valid_to":row.get::<_,Option<String>>(4)?,
                "qualified_tx":{"provider":"main", "source_store_id":source_id, "generation":generation,"local_tx":tx},
                "transaction":store.conn.query_row(
                    "SELECT timestamp,actor,source FROM main.transactions WHERE id=?1", [tx],
                    |r| Ok(json!({"timestamp":r.get::<_,String>(0)?,"actor":r.get::<_,Option<String>>(1)?,"source":r.get::<_,Option<String>>(2)?})),
                )?,
                "authenticated":store.transaction_auth(tx)?,
                "declared":store.transaction_declarations(tx)?,
            });
            bytes += serde_json::to_vec(&entry)
                .map_err(|e| invalid(e.to_string()))?
                .len();
            if bytes > bytes_cap {
                return Err(invalid("history byte cap exceeded; completeness UNKNOWN"));
            }
            entries.push(entry);
        }
        entries
    };
    let result = json!({
        "schema":CONTRACT, "iri":request.iri, "graph":request.graph,
        "source_receipt":{
            "provider":"main", "source_store_id":source_id, "generation":generation,
            "incarnation":store.acquisition_incarnation, "observed_head_tx":head,
            "retained_log_min_tx":floor,"since_tx_exclusive":since,"until_tx_inclusive":until,
            "coverage":"retained-main-physical-facts", "complete":true,
            "continuous_history":false,"historical_policy_proof":false,
            "excluded_attachment_aliases":aliases,
            "producer_version":env!("CARGO_PKG_VERSION"),
            "producer_git_sha":env!("QUIPU_GIT_SHA"),
            "producer_git_dirty":env!("QUIPU_GIT_DIRTY")=="true",
        },
        "count":history.len(),"history":history,
    });
    if serde_json::to_vec(&result)
        .map_err(|e| invalid(e.to_string()))?
        .len()
        > bytes_cap
    {
        return Err(invalid(
            "history response byte cap exceeded; completeness UNKNOWN",
        ));
    }
    budget.check()?;
    snapshot.commit()?;
    Ok(result)
}

#[cfg(test)]
#[path = "history_tests.rs"]
mod tests;
