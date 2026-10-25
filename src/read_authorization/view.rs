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
}

impl AuthorizedRead {
    /// Check exact scope before reading source facts. One source transaction
    /// pins all selected graphs; hidden metadata/dictionaries are not copied.
    pub fn open(self, source: &Store, now: u64) -> Result<ReadView> {
        if now >= self.expiry || now < self.authorized_at || source.has_attachments() {
            return Err(denied());
        }
        let read = source.conn.unchecked_transaction()?;
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
        read.commit()?;
        Ok(ReadView {
            authorization: self,
            store,
            source: source_id,
            epoch,
            graph_ids,
            exports,
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
