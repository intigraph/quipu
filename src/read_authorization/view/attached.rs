//! Phase 2 composed reader over read-only ATTACH (aegis-n0wc.5, seam A,
//! docs/design/per-graph-writable-stores.md "Terms, links and query
//! semantics"). The existing attach composer (`store::attach`, quipu #75)
//! assumes every mounted file allocates ids from its own term space (#74).
//! Nothing in a graph-set file proves that, so the mount proves it first:
//!
//! - R1a: two participant files sharing one term space refuse.
//! - R1c: the space a file DECLARES is not trusted. Its real `terms` id range
//!   is measured and must lie inside that space.
//! - R1d: only the configured legacy file may be in space 0. A space-0 set
//!   file refuses with a reason and is never renumbered on read.
//!
//! The checks live in `store::composition::check_term_spaces`; the dataset
//! runs them BEFORE any ATTACH, and this entry point repeats them.

// Dormant like `GraphSetOwner::open_enrolled`: no production caller until the
// Phase 2 reader is wired to a route (aegis-n0wc.5).
#![cfg_attr(not(test), allow(dead_code))]

use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

use super::{ComposedReadPermit, ComposedReadView, ReadOperation, ReadView, denied};
use crate::error::Result;
use crate::read_authorization::AuthorizedRead;
use crate::store::composition::{Checkout, check_term_spaces};
use crate::store::graph_owner::CapturedRead;

impl AuthorizedRead {
    /// Seam A composed read over one pooled read-only ATTACH connection whose
    /// snapshot was begun under every participant's commit fence. Proves the
    /// term-space preconditions first; copies no fact and opens no scratch.
    /// The owner pilot keeps `open_composed_cut` unchanged.
    pub(crate) fn open_attached_cut(
        self,
        reads: &[CapturedRead<'_>],
        mut checkout: Checkout,
        permit: ComposedReadPermit,
        now: u64,
        deadline: Instant,
    ) -> Result<ComposedReadView> {
        if Instant::now() >= deadline {
            return Err(denied());
        }
        if self.operation != ReadOperation::Query
            || now >= self.expiry
            || now < self.authorized_at
            || reads.is_empty()
        {
            return Err(denied());
        }
        let revision = reads[0].catalog_revision;
        let selected: BTreeSet<_> = reads.iter().map(|r| r.route.graph.clone()).collect();
        // Complete scope check before reading source facts.
        if selected.len() != reads.len() || selected != self.graphs {
            return Err(denied());
        }
        check_term_spaces(reads)?;
        let mut bindings = BTreeMap::new();
        let mut graph_ids = Vec::new();
        let mut main_graphs = Vec::new();
        let mut attached: BTreeMap<String, Vec<i64>> = BTreeMap::new();
        for read in reads {
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
            let graph = if read.route.graph == crate::schema::ROOT_GRAPH_IRI {
                0
            } else {
                source
                    .committed_graph_id(&read.route.graph)?
                    .ok_or_else(denied)?
            };
            if source.graph_class(graph)?.as_deref() != Some("committed") {
                return Err(denied());
            }
            if read.legacy {
                main_graphs.push(graph);
            } else {
                let schema = checkout
                    .schema_for(&read.route.store_id)
                    .ok_or_else(denied)?
                    .to_owned();
                attached.entry(schema).or_default().push(graph);
            }
            graph_ids.push(graph);
            bindings.insert(read.route.graph.clone(), (read.route.clone(), revision));
        }
        if Instant::now() >= deadline {
            return Err(denied());
        }
        checkout.compose(&main_graphs, &attached.into_iter().collect::<Vec<_>>())?;
        let store = checkout.take_store();
        let allocation = permit.allocation();
        if let Err(error) = store.apply_memory_allocation(allocation) {
            checkout.restore(store);
            return Err(error);
        }
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
        Ok(ComposedReadView::attached(
            view, bindings, deadline, checkout, permit,
        ))
    }
}
