//! Exact registry metadata reads and the additive v1 creator-event index.
use super::Store;
use crate::error::{Error, Result};
use rusqlite::{Connection, OptionalExtension};
use serde_json::{Value, json};

const INDEX: &str = "idx_events_type_subject_offset";
const FROZEN_INDEX: &str = "idx_frozen_packs_graph_thawed";

impl Store {
    /// Source-ordered v1 migration. No facts/events change; rollback drops only
    /// its two metadata indexes. Read-only opens never run migrations.
    pub(super) fn migrate_graph_metadata_v1(conn: &Connection) -> Result<()> {
        conn.execute_batch(
            "CREATE INDEX IF NOT EXISTS main.idx_events_type_subject_offset ON events(type,subject,offset);\
             CREATE INDEX IF NOT EXISTS main.idx_frozen_packs_graph_thawed ON frozen_packs(graph_iri,thawed_at);",
        )?;
        require_creator_index(conn, "main")?;
        require_index(conn, "main", FROZEN_INDEX, &["graph_iri", "thawed_at"])
    }

    /// Open a metadata-only reader without migration, graph registration or
    /// lexical initialization. Existing attached/frozen registry identities
    /// must already have been admitted by the writer.
    pub fn open_graph_metadata_reader(
        path: &str,
        declared: &[super::attach::Attachment],
        graph: &str,
    ) -> Result<Self> {
        let mut store = Self::open_read_only(path)?;
        let mut attachments = declared.to_vec();
        oxrdf::NamedNode::new(graph)
            .map_err(|e| Error::InvalidValue(format!("invalid graph IRI: {e}")))?;
        if let Some((alias, path)) = frozen_source(&store.conn, graph)? {
            if !std::path::Path::new(&path).is_file() {
                return Err(Error::Store(
                    "selected frozen metadata source is unavailable".into(),
                ));
            }
            attachments.push(super::attach::Attachment::read_only(&alias, &path));
        }
        super::attach::attach_all(&store.conn, &attachments)?;
        if !attachments.is_empty() {
            super::attach::verify_attached_schema(
                &store.conn,
                &attachments,
                super::local_term_space(&store.conn)?,
            )?;
        }
        store.attachments = attachments;
        Ok(store)
    }

    fn select_metadata_identity(&self, graph: &str, ids: Vec<i64>) -> Result<Vec<i64>> {
        let mut frozen = Vec::new();
        for &g in &ids {
            let (source, lifecycle): (Option<String>, Option<String>) = self.conn.query_row(
                "SELECT source,lifecycle FROM main.graphs WHERE g=?1",
                [g],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            if source.is_none() && lifecycle.as_deref() == Some("frozen") {
                frozen.push(g);
            }
        }
        if frozen.is_empty() {
            return Ok(ids);
        }
        // A frozen graph has a retained local lifecycle row AND a respaced
        // source row. The recorded active freeze, not a mutable actor event,
        // names which attached identity is authoritative for this read.
        let Some((alias, _)) = frozen_source(&self.conn, graph)? else {
            return Err(Error::Store(
                "frozen metadata requires an active source identity".into(),
            ));
        };
        let mut selected = Vec::new();
        for g in ids {
            if frozen.contains(&g) {
                continue;
            }
            let source: Option<String> =
                self.conn
                    .query_row("SELECT source FROM main.graphs WHERE g=?1", [g], |r| {
                        r.get(0)
                    })?;
            if source.as_ref() != Some(&alias) {
                return Err(Error::Store("ambiguous frozen metadata sources".into()));
            }
            selected.push(g);
        }
        if selected.len() != 1 {
            return Err(Error::Store("frozen metadata source is unavailable".into()));
        }
        Ok(selected)
    }

    /// Read exactly one registered IRI, without interning, registry enumeration
    /// or materialization enrichment. Absence is `None`; storage failures are
    /// errors, never an empty successful answer. Attached provenance comes from
    /// its immutable source, never a mutable local event of the same name.
    pub fn graph_metadata(&self, graph: &str) -> Result<Option<Value>> {
        oxrdf::NamedNode::new(graph)
            .map_err(|e| Error::InvalidValue(format!("invalid graph IRI: {e}")))?;
        let ids = if graph == crate::schema::ROOT_GRAPH_IRI {
            vec![0]
        } else {
            self.lookup_all(graph)?
        };
        let mut registered = Vec::new();
        for g in ids {
            if g != self.meta_graph_id()?
                && self
                    .conn
                    .query_row("SELECT g FROM main.graphs WHERE g=?1", [g], |r| {
                        r.get::<_, i64>(0)
                    })
                    .optional()?
                    .is_some()
            {
                registered.push(g);
            }
        }
        let registered = self.select_metadata_identity(graph, registered)?;
        let g = match registered.as_slice() {
            [] => return Ok(None),
            [g] => *g,
            _ => {
                return Err(Error::Store(
                    "graph IRI has multiple registered source identities".into(),
                ));
            }
        };
        let row = self.conn.query_row(
            "SELECT class,source,lifecycle,fresh_rank,durability_rank,trust_rank,trust_chain,policy,data_kind,created_at FROM main.graphs WHERE g=?1",
            [g], |r| Ok(json!({
                "iri":graph,"g":g,"class":r.get::<_,String>(0)?,
                "source":r.get::<_,Option<String>>(1)?,"lifecycle":r.get::<_,Option<String>>(2)?,
                "labels":{
                    "freshness":r.get::<_,Option<i64>>(3)?.map(super::registry_list::rank_to_freshness),
                    "durability":r.get::<_,Option<i64>>(4)?.map(super::registry_list::rank_to_durability),
                    "trust_rank":r.get::<_,Option<i64>>(5)?,"trust_chain":r.get::<_,Option<String>>(6)?,
                    "policy":r.get::<_,Option<String>>(7)?,"kind":r.get::<_,Option<String>>(8)?},
                "creation":{"timestamp":r.get::<_,String>(9)?,"actor":null}
            })),
        ).optional()?;
        let Some(mut row) = row else {
            return Ok(None);
        };
        let schema = row["source"].as_str().unwrap_or("main").to_string();
        if row["source"].is_string() {
            super::attach::validate_alias(&schema)?;
            if !self.attachments.iter().any(|a| a.alias == schema) {
                return Err(Error::Store("graph metadata source is not attached".into()));
            }
            // Anchor both the identity and timestamp in the selected source.
            // A missing source row or a broken pack is UNKNOWN/error.
            let (iri, timestamp): (String, String) = self.conn.query_row(
                &format!("SELECT t.iri,g.created_at FROM {schema}.graphs g JOIN {schema}.terms t ON t.id=g.g WHERE g.g=?1"),
                [g], |r| Ok((r.get(0)?,r.get(1)?)),
            )?;
            if iri != graph {
                return Err(Error::Store(
                    "graph metadata source identity mismatch".into(),
                ));
            }
            row["creation"]["timestamp"] = json!(timestamp);
        }
        let actor = creator(&self.conn, &schema, graph)?;
        row["creation"]["actor"] = actor;
        if self
            .conn
            .query_row(
                "SELECT lifecycle FROM main.graphs WHERE g=?1",
                [self.lookup(graph)?.unwrap_or(-1)],
                |r| r.get::<_, Option<String>>(0),
            )
            .optional()?
            .flatten()
            .as_deref()
            == Some("frozen")
        {
            row["lifecycle"] = json!("frozen");
        }
        Ok(Some(row))
    }
}

fn require_creator_index(conn: &Connection, schema: &str) -> Result<()> {
    require_index(conn, schema, INDEX, &["type", "subject", "offset"])
}

fn require_index(conn: &Connection, schema: &str, index: &str, expected: &[&str]) -> Result<()> {
    let mut stmt = conn.prepare(&format!("PRAGMA {schema}.index_info('{index}')"))?;
    let cols = stmt
        .query_map([], |r| r.get::<_, String>(2))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    if cols != expected {
        return Err(Error::Store(format!(
            "graph metadata requires source index v1 {index}; source is not migrated by this read"
        )));
    }
    Ok(())
}

fn frozen_source(conn: &Connection, graph: &str) -> Result<Option<(String, String)>> {
    require_index(conn, "main", FROZEN_INDEX, &["graph_iri", "thawed_at"])?;
    let mut stmt=conn.prepare("SELECT alias,path FROM main.frozen_packs INDEXED BY idx_frozen_packs_graph_thawed WHERE graph_iri=?1 AND thawed_at IS NULL LIMIT 2")?;
    let rows = stmt
        .query_map([graph], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    match rows.len() {
        0 => Ok(None),
        1 => Ok(rows.into_iter().next()),
        _ => Err(Error::Store(
            "frozen metadata has multiple active source identities".into(),
        )),
    }
}

fn creator(conn: &Connection, schema: &str, graph: &str) -> Result<Value> {
    require_creator_index(conn, schema)?;
    let payload: Option<String> = conn.query_row(
        &format!("SELECT payload FROM {schema}.events INDEXED BY {INDEX} WHERE type='graph.created' AND subject=?1 ORDER BY offset DESC LIMIT 1"),
        [graph], |r| r.get(0),
    ).optional()?;
    match payload {
        None => Ok(Value::Null),
        Some(payload) => {
            let value: Value = serde_json::from_str(&payload)
                .map_err(|e| Error::Store(format!("invalid graph creation record: {e}")))?;
            if !value.is_object() {
                return Err(Error::Store(
                    "graph creation record must be an object".into(),
                ));
            }
            if let Some(record_graph) = value.get("graph")
                && record_graph.as_str() != Some(graph)
            {
                return Err(Error::Store(
                    "graph creation record identity mismatch".into(),
                ));
            }
            match value.get("actor") {
                None | Some(Value::Null) => Ok(Value::Null),
                Some(Value::String(actor)) => Ok(json!(actor)),
                _ => Err(Error::Store("invalid graph creation actor".into())),
            }
        }
    }
}

#[cfg(test)]
#[path = "graph_metadata_tests.rs"]
mod tests;
