//! A physically qualified, bounded, current-state view. No unscoped Store is
//! retained or exposed to computations. History, attachments, stored datasets,
//! external services, search indexes and inference engines are unsupported.

use std::collections::BTreeMap;

use spargebra::{Query, term::NamedNodePattern};

use super::{AuthorizedRead, ReadOperation, denied};
use crate::{Result, Store, sparql};

/// Upper bounds, not paging: a partial view is never returned as complete.
const MAX_VIEW_BYTES: usize = 16 * 1024 * 1024;
const MAX_VIEW_FACTS: usize = 100_000;

/// Request-owned state. SQL, resident models, term dictionaries and caches are
/// built only from authorized current facts. No raw connection/store accessor.
///
/// ```compile_fail
/// fn tenant(view: &quipu::read_authorization::ReadView) {
///     let unscoped_store = &view.store;
/// }
/// ```
pub struct ReadView {
    authorization: AuthorizedRead,
    store: Store,
    source: String,
    epoch: String,
    graph_ids: Vec<i64>,
    exports: BTreeMap<String, String>,
    #[cfg(all(feature = "ha-read-profile", target_os = "linux"))]
    native_ha: bool,
}

impl AuthorizedRead {
    /// Check exact scope before reading source facts. One source transaction
    /// pins all selected graphs; hidden metadata/dictionaries are not copied.
    pub fn open(self, source: &Store, now: u64) -> Result<ReadView> {
        self.open_with_snapshot(source, now, false, false)
    }

    // The HA profile pins cursor, eligibility and this copy in ONE caller-owned
    // read transaction. Keep the ordinary API's transaction ownership unchanged.
    #[cfg(all(feature = "ha-read-profile", target_os = "linux"))]
    pub(crate) fn open_in_snapshot(self, source: &Store, now: u64) -> Result<ReadView> {
        if source.conn.is_autocommit() {
            return Err(denied());
        }
        self.open_with_snapshot(source, now, true, true)
    }

    fn open_with_snapshot(
        self,
        source: &Store,
        now: u64,
        borrowed: bool,
        native: bool,
    ) -> Result<ReadView> {
        if now >= self.expiry || now < self.authorized_at || source.has_attachments() {
            return Err(denied());
        }
        let read = if borrowed {
            None
        } else {
            Some(source.conn.unchecked_transaction()?)
        };
        let source_id = source.store_id()?;
        let epoch = source.log_epoch()?;
        let mut store = Store::open_in_memory()?;
        let mut graph_ids = Vec::new();
        let mut exports = BTreeMap::new();
        let mut bytes = 0usize;
        let mut facts = 0usize;
        for graph in &self.graphs {
            let root = graph == crate::schema::ROOT_GRAPH_IRI;
            let g = if root {
                0
            } else {
                source.committed_graph_id(graph)?.ok_or_else(denied)?
            };
            // Implicit overlay, frozen/attached or dataset expansion is absent.
            if source.is_dataset(graph)? || source.graph_class(g)?.as_deref() != Some("committed") {
                return Err(denied());
            }
            #[cfg(all(feature = "ha-read-profile", target_os = "linux"))]
            if native {
                if self.operation != ReadOperation::Query {
                    return Err(denied());
                }
                let destination = if root { 0 } else { store.graph_create(graph)? };
                copy_native(source, &mut store, g, destination, &mut facts, &mut bytes)?;
                graph_ids.push(destination);
                continue;
            }
            #[cfg(not(all(feature = "ha-read-profile", target_os = "linux")))]
            let _ = native;
            let (rows, value_bytes): (i64, i64) = source.conn.query_row(
                "SELECT COUNT(*), COALESCE(SUM(length(v)), 0) FROM facts WHERE g=?1 AND op=1 AND valid_to IS NULL",
                [g], |row| Ok((row.get(0)?, row.get(1)?)),
            )?;
            let rows = usize::try_from(rows).map_err(|_| denied())?;
            let value_bytes = usize::try_from(value_bytes).map_err(|_| denied())?;
            if rows > MAX_VIEW_FACTS || value_bytes > MAX_VIEW_BYTES {
                return Err(denied());
            }
            let (rdf, count) = crate::rdf::export_rdf_subset(
                source,
                oxrdfio::RdfFormat::NTriples,
                (!root).then_some(graph.as_str()),
            )?;
            bytes = bytes.checked_add(rdf.len()).ok_or_else(denied)?;
            facts = facts.checked_add(count).ok_or_else(denied)?;
            if bytes > MAX_VIEW_BYTES || facts > MAX_VIEW_FACTS {
                return Err(denied());
            }
            // Stable allowed-data ordering makes hidden term allocation/history
            // unable to perturb the scratch dictionary or serialized outputs.
            let text = String::from_utf8(rdf).map_err(|_| denied())?;
            let mut lines: Vec<_> = text.lines().collect();
            lines.sort_unstable();
            let canonical = if lines.is_empty() {
                String::new()
            } else {
                format!("{}\n", lines.join("\n"))
            };
            let destination = if root { 0 } else { store.graph_create(graph)? };
            crate::rdf::ingest_assigned_rdf(
                &mut store,
                canonical.as_bytes(),
                oxrdfio::RdfFormat::NTriples,
                None,
                "1970-01-01T00:00:00Z",
                None,
                None,
                destination,
            )?;
            graph_ids.push(destination);
            exports.insert(graph.clone(), canonical);
        }
        if let Some(read) = read {
            read.commit()?;
        }
        Ok(ReadView {
            authorization: self,
            store,
            source: source_id,
            epoch,
            graph_ids,
            exports,
            #[cfg(all(feature = "ha-read-profile", target_os = "linux"))]
            native_ha: native,
        })
    }
}

impl ReadView {
    fn require(&self, operation: ReadOperation, now: u64) -> Result<()> {
        if self.authorization.operation != operation
            || now >= self.authorization.expiry
            || now < self.authorization.authorized_at
        {
            return Err(denied());
        }
        Ok(())
    }

    /// This binding is for trusted adapters, never an authority to open a source.
    pub fn source_binding(&self) -> (&str, &str) {
        (&self.source, &self.epoch)
    }

    /// A changed source lineage/epoch never validates a view of the old source.
    pub fn recheck_source(&self, source: &Store) -> Result<()> {
        if source.store_id()? != self.source || source.log_epoch()? != self.epoch {
            return Err(denied());
        }
        Ok(())
    }

    /// Revalidate fresh policy and identity immediately before publication.
    pub fn recheck(
        &self,
        policy: &super::ReadPolicy,
        identity: &super::ReadIdentity,
        now: u64,
    ) -> Result<()> {
        self.authorization.recheck(policy, identity, now)
    }

    /// Execute only after preflighting every GRAPH/FROM/SERVICE occurrence,
    /// including scopes hidden in EXISTS, subqueries and skipped branches.
    /// Caller execution context, stored datasets, inference and attachments
    /// cannot be injected: this API accepts only query text.
    pub fn query(&self, text: &str, resident_model: bool, now: u64) -> Result<Vec<u8>> {
        self.query_response(text, resident_model, now)
            .map(|(_, bytes)| bytes)
    }

    /// The serializer supplies its actual media type; callers never sniff data.
    pub fn query_response(
        &self,
        text: &str,
        resident_model: bool,
        now: u64,
    ) -> Result<(&'static str, Vec<u8>)> {
        self.require(ReadOperation::Query, now)?;
        let parsed = crate::sparql_structure::parse_query(sparql::sparql_parser(), text)?
            .map_err(|_| denied())?;
        #[cfg(all(feature = "ha-read-profile", target_os = "linux"))]
        if self.native_ha {
            sparql::root_scope::refuse_custom_functions(&parsed)?;
        }
        let dataset = match &parsed {
            Query::Select { dataset, .. }
            | Query::Ask { dataset, .. }
            | Query::Construct { dataset, .. }
            | Query::Describe { dataset, .. } => dataset,
        };
        if let Some(dataset) = dataset
            && (dataset
                .default
                .iter()
                .any(|g| !self.authorization.graphs.contains(g.as_str()))
                || dataset.named.as_ref().is_some_and(|graphs| {
                    graphs
                        .iter()
                        .any(|g| !self.authorization.graphs.contains(g.as_str()))
                }))
        {
            return Err(denied());
        }
        sparql::root_scope::visit_scopes(&parsed, |scope| match scope {
            sparql::root_scope::Scope::Service => Err(denied()),
            sparql::root_scope::Scope::Graph(NamedNodePattern::NamedNode(graph))
                if !self.authorization.graphs.contains(graph.as_str()) =>
            {
                Err(denied())
            }
            sparql::root_scope::Scope::Graph(_) => Ok(()),
        })?;
        self.store.set_read_model_enabled(resident_model);
        let context = sparql::TemporalContext {
            graph: sparql::GraphScope::Default(self.graph_ids.clone()),
            named_dataset: Some(self.graph_ids.iter().copied().filter(|g| *g != 0).collect()),
            ..Default::default()
        };
        let result = sparql::query_temporal(&self.store, text, &context)?;
        let format = match result {
            sparql::QueryResult::Select { .. } | sparql::QueryResult::Ask(_) => {
                crate::w3c::ResultFormat::SparqlJson
            }
            sparql::QueryResult::Graph(_) => crate::w3c::ResultFormat::NTriples,
        };
        crate::w3c::serialize(&self.store, &result, format)?.ok_or_else(denied)
    }

    /// A complete current graph export/acquisition, with no global transaction,
    /// actor, history or hidden registry fields. Not an R3 bootstrap receipt.
    pub fn export(&self, now: u64) -> Result<&BTreeMap<String, String>> {
        self.require(ReadOperation::Export, now)?;
        Ok(&self.exports)
    }

    /// Only authorized graph names and fact counts; no global head or labels.
    pub fn metadata(&self, now: u64) -> Result<BTreeMap<String, usize>> {
        self.require(ReadOperation::Metadata, now)?;
        Ok(self
            .exports
            .iter()
            .map(|(graph, text)| (graph.clone(), text.lines().count()))
            .collect())
    }
}

// Query-only HA views retain native bytes and valid intervals. Reuse the SAME
// lossless record encoder/converter as cut/tail; this is not another applier.
// Export/metadata retain their existing RDF-only contract and ordinary path.
#[cfg(all(feature = "ha-read-profile", target_os = "linux"))]
fn copy_native(
    source: &Store,
    destination: &mut Store,
    graph: i64,
    target: i64,
    facts: &mut usize,
    bytes: &mut usize,
) -> Result<()> {
    let mut stmt = source.conn.prepare(
        "SELECT f.e,f.a,CASE WHEN length(f.v)<=?2 THEN f.v ELSE NULL END,f.valid_from,f.valid_to,length(e.iri),length(a.iri) FROM facts f LEFT JOIN terms e ON e.id=f.e LEFT JOIN terms a ON a.id=f.a WHERE f.g=?1 AND f.op=1 AND f.retracted_tx IS NULL LIMIT 100001")?;
    let mut rows = stmt.query(rusqlite::params![
        graph,
        i64::try_from(MAX_VIEW_BYTES).map_err(|_| denied())?
    ])?;
    let mut records = Vec::new();
    while let Some(row) = rows.next()? {
        *facts = facts.checked_add(1).ok_or_else(denied)?;
        if *facts > MAX_VIEW_FACTS {
            return Err(denied());
        }
        let value = row.get::<_, Option<Vec<u8>>>(2)?.ok_or_else(denied)?;
        if row.get::<_, i64>(5)? > 8192 || row.get::<_, i64>(6)? > 8192 {
            return Err(denied());
        }
        let decoded = crate::Value::from_bytes(&value)?;
        if let crate::Value::Ref(id) = decoded {
            let bounded: bool = source.conn.query_row(
                "SELECT length(iri)<=8192 FROM terms WHERE id=?1",
                [id],
                |r| r.get(0),
            )?;
            if !bounded {
                return Err(denied());
            }
        }
        let e: i64 = row.get(0)?;
        let a: i64 = row.get(1)?;
        let record = serde_json::json!({"entity":source.resolve(e)?,"attribute":source.resolve(a)?,
            "value":source.value_json(&decoded)?,"op":"assert",
            "valid_from":row.get::<_,String>(3)?,"valid_to":row.get::<_,Option<String>>(4)?});
        let encoded = serde_json::to_string(&record).map_err(|_| denied())?;
        *bytes = bytes.checked_add(encoded.len()).ok_or_else(denied)?;
        if *bytes > MAX_VIEW_BYTES {
            return Err(denied());
        }
        records.push((encoded, record));
    }
    records.sort_unstable_by(|a, b| a.0.cmp(&b.0));
    let datums: Vec<_> = records
        .iter()
        .map(|(_, r)| {
            crate::applier::record_to_datum(destination, r, "1970-01-01T00:00:00Z")?
                .map_err(|_| denied())
        })
        .collect::<Result<_>>()?;
    destination.transact_to_graph(&datums, "1970-01-01T00:00:00Z", None, None, target)?;
    Ok(())
}
