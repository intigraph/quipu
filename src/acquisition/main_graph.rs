//! A direct MAIN-only reader: never consult composed dictionaries or models.
use super::invalid;
use crate::{Result, Store};
use oxrdf::{BlankNode, NamedNode, NamedOrBlankNode, Term, Triple};
use rusqlite::{OptionalExtension, params};

pub(super) fn term(store: &Store, id: i64) -> Result<String> {
    if store.has_attachments()
        && store
            .conn
            .prepare("SELECT 1 FROM temp.term_alias WHERE alias_id=?1 AND canonical_id<>?1")?
            .exists([id])?
    {
        return Err(invalid("selected identity overlaps a composed alias"));
    }
    let iri: Option<Option<String>> = store
        .conn
        .query_row(
            "SELECT CASE WHEN length(iri)<=8192 THEN iri ELSE NULL END FROM main.terms WHERE id=?1",
            [id],
            |r| r.get(0),
        )
        .optional()?;
    iri.flatten()
        .ok_or_else(|| invalid("selected identity is not owned by main"))
}

fn subject(iri: &str) -> Result<NamedOrBlankNode> {
    if let Some(label) = iri.strip_prefix(crate::rdf::BLANK_PREFIX) {
        Ok(BlankNode::new(label)
            .map_err(|e| invalid(e.to_string()))?
            .into())
    } else {
        Ok(NamedNode::new(iri)
            .map_err(|e| invalid(e.to_string()))?
            .into())
    }
}

type GraphMetadata = (
    i64,
    String,
    Option<String>,
    Option<String>,
    Option<i64>,
    Option<i64>,
    Option<i64>,
    Option<String>,
    Option<String>,
    Option<i64>,
    Option<String>,
    Option<String>,
);

pub(super) fn metadata(store: &Store, iri: &str) -> Result<(i64, String)> {
    if iri.len()>8192 || store.conn.prepare("SELECT 1 FROM main.graphs g JOIN main.terms t ON t.id=g.g WHERE t.iri=?1 AND COALESCE(length(g.source),0)+COALESCE(length(g.trust_chain),0)+COALESCE(length(g.policy),0)+COALESCE(length(g.data_kind),0)+COALESCE(length(g.labels_valid_to),0)>65536")?.exists([iri])? { return Err(invalid("graph metadata byte cap exceeded")); }
    let row: Option<GraphMetadata> = store.conn.query_row(
        "SELECT t.id,g.class,g.source,g.lifecycle,g.labels_tx,g.fresh_rank,g.trust_rank,g.trust_chain,g.policy,g.durability_rank,g.data_kind,g.labels_valid_to FROM main.terms t JOIN main.graphs g ON g.g=t.id WHERE t.iri=?1",
        [iri], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?,r.get(8)?,r.get(9)?,r.get(10)?,r.get(11)?)),
    ).optional()?;
    let Some(row) = row else {
        return Err(invalid("selected graph is not registered in main"));
    };
    if row.1 != "committed"
        || row.2.is_some()
        || row.3.as_deref() == Some("frozen")
        || store.is_dataset(iri)?
    {
        return Err(invalid(
            "selected scope overlaps an attachment, alias, overlay or frozen graph",
        ));
    }
    if !store.labels_config.is_unset() && (row.6.is_some() || row.7.is_some() || row.11.is_some()) {
        return Err(invalid(
            "time-dependent or trust-derived label floor is unsupported by the direct-main profile",
        ));
    }
    term(store, row.0)?;
    store.check_label_floor(&[row.0])?;
    Ok((
        row.0,
        serde_json::to_string(&row).map_err(|e| invalid(e.to_string()))?,
    ))
}

pub(super) fn read(
    store: &Store,
    graph: i64,
    tx: i64,
    max_bytes: usize,
    check: &impl Fn() -> Result<()>,
) -> Result<String> {
    let mut q = store.conn.prepare(
        "SELECT DISTINCT e,a,CASE WHEN length(v)<=?3 THEN v ELSE NULL END FROM main.facts WHERE g=?1 AND op=1 AND tx<=?2 AND (valid_to IS NULL OR retracted_tx>?2) LIMIT 100001"
    )?;
    let sql_bytes = i64::try_from(max_bytes).map_err(|_| invalid("invalid graph byte budget"))?;
    let mut rows = q.query(params![graph, tx, sql_bytes])?;
    let mut out = String::new();
    let mut count = 0;
    while let Some(row) = rows.next()? {
        check()?;
        count += 1;
        if count > 100000 {
            return Err(invalid(
                "acquisition row cap exceeded; completeness UNKNOWN",
            ));
        }
        let e: i64 = row.get(0)?;
        let a: i64 = row.get(1)?;
        let v: Vec<u8> = row
            .get::<_, Option<Vec<u8>>>(2)?
            .ok_or_else(|| invalid("acquisition byte cap exceeded; completeness UNKNOWN"))?;
        let value = crate::Value::from_bytes(&v)?;
        let object: Term = if let crate::Value::Ref(id) = value {
            subject(&term(store, id)?)?.into()
        } else {
            crate::rdf::value_to_term(store, &value)?
        };
        let triple = Triple {
            subject: subject(&term(store, e)?)?,
            predicate: NamedNode::new(term(store, a)?).map_err(|e| invalid(e.to_string()))?,
            object,
        };
        out.push_str(&format!("{triple} .\n"));
        if out.len() > max_bytes {
            return Err(invalid("acquisition bytes exceeded; completeness UNKNOWN"));
        }
    }
    let mut lines: Vec<_> = out.lines().collect();
    lines.sort_unstable();
    lines.dedup();
    Ok(if lines.is_empty() {
        String::new()
    } else {
        format!("{}\n", lines.join("\n"))
    })
}
