//! Immutable routing admission for per-graph owners.
//!
//! This module opens no files and grants no writer capability. A selected route
//! is a pin the native owner must verify against its actual file, identity,
//! epoch, policy and exclusive fence. Publishing a catalog in the server is
//! separately gated on mutation-path fencing and composed-read qualification.

use std::collections::{BTreeMap, BTreeSet};

use crate::{Error, Result};

/// Operator-supplied, immutable identity of one graph's native owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Route {
    /// Exact logical graph IRI. Never used to derive a path or entity namespace.
    pub graph: String,
    /// Opaque file identifier, resolved under a separately admitted store root.
    pub file_id: String,
    /// Durable store lineage, distinct from both the file name and graph IRI.
    pub store_id: String,
    /// Change-log epoch expected by this generation.
    pub log_epoch: String,
    /// Monotone operator-published generation of this graph's owner.
    pub generation: u64,
    /// Revision/digest of the policy installed on this owner, not a graph grant.
    pub policy: String,
}

/// Reservations are per owner; their aggregate must fit before publication.
#[derive(Debug, Clone, Copy)]
pub struct Budget {
    /// Upper bound on routed owners; excludes the legacy owner.
    pub max_owners: usize,
    /// FIFO mailbox slots reserved for each owner.
    pub queued_per_owner: usize,
    /// Dedicated read-only connections reserved for each owner.
    pub readers_per_owner: usize,
    /// Aggregate mailbox slots available after legacy reservations.
    pub max_queued: usize,
    /// Includes one writer, its ownership-fence reader and reserved application
    /// readers for every routed owner.
    /// Legacy connections must already have been subtracted by the caller.
    pub max_connections: usize,
}

/// Construct once, pin by reference/Arc for a request; commits never mutate it.
#[derive(Debug)]
pub struct Catalog {
    revision: u64,
    routes: BTreeMap<String, Route>,
    budget: Budget,
}

/// The first pilot supports one explicitly named graph per write.
#[derive(Debug, PartialEq, Eq)]
pub enum WriteTarget<'a> {
    /// Preserve the ordinary legacy validation and mutation path.
    Legacy,
    /// Exact route, still requiring native identity/fence verification.
    Routed(&'a Route),
}

/// Request/consumer pins, checked before a queue slot or writable connection.
pub struct WriteRequest<'a> {
    /// Explicit destination; None means the legacy default graph.
    pub graph: Option<&'a str>,
    /// Catalog captured by this request before admission.
    pub catalog_revision: u64,
    /// Required for a routed graph; changing a route invalidates old requests.
    pub owner_generation: Option<u64>,
    /// Expected installed owner policy revision/digest.
    pub policy: &'a str,
    /// Signed replay currently belongs to the legacy authoritative store.
    pub signed: bool,
    /// Synchronous context outside the selected graph needs a future contract.
    pub cross_graph_context: bool,
}

fn refuse(message: impl Into<String>) -> Error {
    Error::PolicyDenied(format!("graph owner admission: {}", message.into()))
}

fn nonempty(value: &str) -> bool {
    !value.trim().is_empty() && value == value.trim()
}

impl Catalog {
    /// Validate the whole candidate before it can be pinned by any request.
    /// Duplicate physical inodes and filesystem placement are additionally
    /// checked by the native owner; distinct opaque IDs alone cannot prove that.
    pub fn new(
        revision: u64,
        legacy_store_id: &str,
        routes: Vec<Route>,
        budget: Budget,
    ) -> Result<Self> {
        if revision == 0 || !nonempty(legacy_store_id) {
            return Err(refuse("missing catalog revision or legacy store identity"));
        }
        if budget.max_owners == 0
            || budget.queued_per_owner == 0
            || budget.readers_per_owner == 0
            || routes.len() > budget.max_owners
        {
            return Err(refuse("invalid or exceeded per-owner resource reservation"));
        }
        let queued = routes.len().checked_mul(budget.queued_per_owner);
        let connections = budget
            .readers_per_owner
            .checked_add(2)
            .and_then(|per_owner| routes.len().checked_mul(per_owner));
        if queued.is_none_or(|n| n > budget.max_queued)
            || connections.is_none_or(|n| n > budget.max_connections)
        {
            return Err(refuse("aggregate owner resource budget exceeded"));
        }
        let mut by_graph = BTreeMap::new();
        let mut files = BTreeSet::new();
        let mut stores = BTreeSet::from([legacy_store_id.to_owned()]);
        for route in routes {
            if oxrdf::NamedNode::new(route.graph.as_str()).is_err()
                || !nonempty(&route.graph)
                || route.graph == "urn:quipu:graph:root"
            {
                return Err(refuse("route requires a non-ROOT absolute graph IRI"));
            }
            if route.file_id.is_empty()
                || route.file_id.len() > 64
                || !route
                    .file_id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
            {
                return Err(refuse("file identifier is not an opaque bounded name"));
            }
            if route.generation == 0
                || !nonempty(&route.store_id)
                || !nonempty(&route.log_epoch)
                || !nonempty(&route.policy)
            {
                return Err(refuse(
                    "route lacks an identity, epoch, generation or policy pin",
                ));
            }
            if !files.insert(route.file_id.clone()) || !stores.insert(route.store_id.clone()) {
                return Err(refuse("file or store has more than one writer route"));
            }
            if by_graph.insert(route.graph.clone(), route).is_some() {
                return Err(refuse("graph has more than one owner generation"));
            }
        }
        Ok(Self {
            revision,
            routes: by_graph,
            budget,
        })
    }

    /// Revision pinned by every admitted request.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Aggregate/per-owner reservations checked at construction.
    pub fn budget(&self) -> Budget {
        self.budget
    }

    /// Routes in deterministic graph order; no mutable catalog access exists.
    pub fn routes(&self) -> impl Iterator<Item = &Route> {
        self.routes.values()
    }

    /// Select only; no file open, fallback, queue admission or staging occurs.
    /// An unrouted graph retains legacy validation (including unknown-graph
    /// refusal) at the ordinary write pipeline; this is not graph authorization.
    pub fn write_target(&self, request: &WriteRequest<'_>) -> Result<WriteTarget<'_>> {
        if request.catalog_revision != self.revision {
            return Err(refuse("stale catalog revision"));
        }
        // The legacy knot parser trims graph input. Do not let an alternate
        // spelling miss this catalog and subsequently hit a routed graph in
        // the legacy writer after that downstream normalization.
        if request.graph.is_some_and(|graph| !nonempty(graph)) {
            return Err(refuse("graph target must be an exact nonblank IRI"));
        }
        let Some(route) = request.graph.and_then(|g| self.routes.get(g)) else {
            if request.owner_generation.is_some() {
                return Err(refuse("owner pin does not name a registered route"));
            }
            return Ok(WriteTarget::Legacy);
        };
        if request.owner_generation != Some(route.generation) {
            return Err(refuse("missing or stale owner generation"));
        }
        if request.policy != route.policy {
            return Err(refuse("owner policy pin differs"));
        }
        if request.signed || request.cross_graph_context {
            return Err(refuse(
                "routed signed replay or cross-graph validation is unsupported",
            ));
        }
        Ok(WriteTarget::Routed(route))
    }

    /// The pilot has no multi-file atomic commit. Refuse the ENTIRE batch,
    /// including legacy+routed mixtures, before selecting its first writer.
    pub fn write_batch(&self, requests: &[WriteRequest<'_>]) -> Result<WriteTarget<'_>> {
        let Some(first) = requests.first() else {
            return Err(refuse("empty write batch"));
        };
        let target = self.write_target(first)?;
        for request in &requests[1..] {
            if self.write_target(request)? != target {
                return Err(refuse("multi-file write batch is unsupported"));
            }
        }
        Ok(target)
    }
}

#[cfg(test)]
#[path = "graph_catalog_tests.rs"]
mod tests;
