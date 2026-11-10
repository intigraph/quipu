//! Private pilot composition through authorized, already captured owner snapshots.

use std::{
    collections::{BTreeMap, BTreeSet},
    time::Instant,
};

use super::{AuthorizedRead, MAX_VIEW_BYTES, MAX_VIEW_FACTS, ReadOperation, ReadView, denied};
use crate::{Result, Store, store::graph_owner::CapturedRead};

/// Request-owned authorized RDF view of one strict owner cut. This API does
/// not install a route or reserve evaluator/RSS capacity. The trusted caller
/// must admit its scratch connection and heap lifetime before opening the view.
pub struct ComposedReadView {
    view: ReadView,
    bindings: BTreeMap<String, (crate::graph_catalog::Route, u64)>,
    deadline: Instant,
    // Drop scratch state before releasing its connection reservation.
    _permit: super::ComposedReadPermit,
}

impl AuthorizedRead {
    /// Copy exactly the authorized graphs from an already captured strict cut.
    /// No current-read fallback, attachment, history or implicit ROOT expansion.
    /// The caller reserves from the SAME service-sized admission handle BEFORE
    /// strict capture; the view owns that permit for its scratch lifetime.
    pub fn open_composed_cut(
        self,
        reads: &[CapturedRead<'_>],
        permit: super::ComposedReadPermit,
        now: u64,
        deadline: Instant,
    ) -> Result<ComposedReadView> {
        let check = || {
            if Instant::now() >= deadline {
                Err(denied())
            } else {
                Ok(())
            }
        };
        check()?;
        let allocation = permit.allocation();
        if self.operation != ReadOperation::Query
            || now >= self.expiry
            || now < self.authorized_at
            || reads.is_empty()
            || allocation.connections < 2
        {
            return Err(denied());
        }
        let revision = reads[0].catalog_revision;
        let selected: BTreeSet<_> = reads.iter().map(|r| r.route.graph.clone()).collect();
        // Complete scope check before reading source facts or opening scratch.
        if selected.len() != reads.len() || selected != self.graphs {
            return Err(denied());
        }
        let mut bindings = BTreeMap::new();
        let mut graphs = Vec::new();
        for read in reads {
            check()?;
            let source = read.store;
            if !permit.matches(read)
                || read.catalog_revision != revision
                || source.conn.is_autocommit()
                || source.has_attachments()
                || source.store_id()? != read.route.store_id
                || source.log_epoch()? != read.route.log_epoch
                || source.is_dataset(&read.route.graph)?
            {
                return Err(denied());
            }
            let graph = source
                .committed_graph_id(&read.route.graph)?
                .ok_or_else(denied)?;
            if source.graph_class(graph)?.as_deref() != Some("committed") {
                return Err(denied());
            }
            graphs.push(graph);
            bindings.insert(read.route.graph.clone(), (read.route.clone(), revision));
        }
        let mut store = Store::open_in_memory()?;
        store.apply_memory_allocation(allocation)?;
        let mut graph_ids = Vec::new();
        let mut facts = 0usize;
        let mut bytes = 0usize;
        for (read, graph) in reads.iter().zip(graphs) {
            let text = bounded_graph(read.store, graph, &mut facts, &mut bytes, deadline)?;
            check()?;
            let destination = store.graph_create(&read.route.graph)?;
            crate::rdf::ingest_assigned_rdf(
                &mut store,
                text.as_bytes(),
                oxrdfio::RdfFormat::NTriples,
                None,
                "1970-01-01T00:00:00Z",
                None,
                None,
                destination,
            )?;
            graph_ids.push(destination);
        }
        check()?;
        // The wrapped single-source binding is private and is never exposed:
        // composed source checks use ALL exact bindings below.
        let view = ReadView {
            authorization: self,
            store,
            source: String::new(),
            epoch: String::new(),
            graph_ids,
            exports: Default::default(),
            #[cfg(all(feature = "ha-read-profile", target_os = "linux"))]
            native_ha: false,
        };
        Ok(ComposedReadView {
            view,
            bindings,
            deadline,
            _permit: permit,
        })
    }
}

impl ComposedReadView {
    /// Reuse the existing recursive GRAPH/FROM/SERVICE authorization preflight
    /// and serializer over one scratch dictionary, never merge result sets.
    pub fn query(&self, text: &str, resident_model: bool, now: u64) -> Result<Vec<u8>> {
        if Instant::now() >= self.deadline {
            return Err(denied());
        }
        let bytes = self.view.query(text, resident_model, now)?;
        if Instant::now() >= self.deadline {
            return Err(denied());
        }
        Ok(bytes)
    }

    /// Fresh policy, identity and complete current owner-generation check before
    /// buffered publication. No single-store binding can certify this view.
    pub fn recheck(
        &self,
        policy: &crate::read_authorization::ReadPolicy,
        identity: &crate::read_authorization::ReadIdentity,
        reads: &[CapturedRead<'_>],
        now: u64,
    ) -> Result<()> {
        if Instant::now() >= self.deadline {
            return Err(denied());
        }
        self.view.recheck(policy, identity, now)?;
        if reads.len() != self.bindings.len() {
            return Err(denied());
        }
        let mut observed = BTreeMap::new();
        for read in reads {
            if read.store.conn.is_autocommit()
                || read.store.has_attachments()
                || read.store.store_id()? != read.route.store_id
                || read.store.log_epoch()? != read.route.log_epoch
            {
                return Err(denied());
            }
            observed.insert(
                read.route.graph.clone(),
                (read.route.clone(), read.catalog_revision),
            );
        }
        if observed.len() != reads.len() || observed != self.bindings {
            return Err(denied());
        }
        Ok(())
    }
}

// Bound each input BEFORE materializing values or resolving dictionary strings.
// Do not call current_facts_in_graph/export_rdf_subset: both collect full Vecs.
// Physical claims count conservatively; canonical lines deduplicate RDF terms.
fn bounded_graph(
    source: &Store,
    graph: i64,
    facts: &mut usize,
    bytes: &mut usize,
    deadline: Instant,
) -> Result<String> {
    const MAX_TERM: i64 = 4096;
    const MAX_VALUE: i64 = 65_536;
    let mut stmt = source.conn.prepare(
        "SELECT f.e,f.a,CASE WHEN length(f.v)<=?2 THEN f.v END,length(f.v),\
         (SELECT length(CAST(iri AS BLOB)) FROM terms WHERE id=f.e),\
         (SELECT length(CAST(iri AS BLOB)) FROM terms WHERE id=f.a) \
         FROM facts f WHERE f.g=?1 AND f.op=1 AND f.valid_to IS NULL LIMIT 100001",
    )?;
    let mut rows = stmt.query(rusqlite::params![graph, MAX_VALUE])?;
    let mut lines = BTreeSet::new();
    while let Some(row) = rows.next()? {
        *facts = facts.checked_add(1).ok_or_else(denied)?;
        if Instant::now() >= deadline || *facts > MAX_VIEW_FACTS {
            return Err(denied());
        }
        for index in [4, 5] {
            if !(1..=MAX_TERM).contains(&row.get::<_, i64>(index)?) {
                return Err(denied());
            }
        }
        if !(1..=MAX_VALUE).contains(&row.get::<_, i64>(3)?) {
            return Err(denied());
        }
        let value = crate::Value::from_bytes(&row.get::<_, Vec<u8>>(2)?)?;
        if let crate::Value::Ref(id) = &value {
            let length: i64 = source.conn.query_row(
                "SELECT length(CAST(iri AS BLOB)) FROM terms WHERE id=?1",
                [id],
                |r| r.get(0),
            )?;
            if !(1..=MAX_TERM).contains(&length) {
                return Err(denied());
            }
        }
        let fact = crate::Fact {
            entity: row.get(0)?,
            attribute: row.get(1)?,
            value,
            tx: 0,
            valid_from: String::new(),
            valid_to: None,
            op: crate::Op::Assert,
        };
        let line = crate::rdf::serialize_facts(source, &[fact], oxrdfio::RdfFormat::NTriples)?;
        *bytes = bytes.checked_add(line.len()).ok_or_else(denied)?;
        if *bytes > MAX_VIEW_BYTES {
            return Err(denied());
        }
        lines.insert(String::from_utf8(line).map_err(|_| denied())?);
    }
    Ok(lines.into_iter().collect())
}
