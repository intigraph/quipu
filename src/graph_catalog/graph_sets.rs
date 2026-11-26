//! Explicit graph-set planning; no writer, route publication or migration.
//!
//! Physical owners are reserved once, while every logical graph remains an
//! exact selection. The existing single-graph Catalog contract is unchanged.
use super::*;

/// Deployment-supplied graphs that must share one file and atomic writer.
pub struct GraphSet {
    /// Canonical physical-owner pin; its graph must also occur in `graphs`.
    pub owner: Route,
    /// Complete, explicit logical membership. No prefix or suffix expansion.
    pub graphs: Vec<String>,
}

/// One physical owner and the exact logical graphs selected on it.
/// This is a plan, never authorization or a writable capability.
#[derive(Debug)]
pub struct OwnerSelection<'a> {
    /// Immutable physical-owner identity, epoch, policy and generation.
    pub owner: &'a Route,
    /// Selected graphs only, in canonical order.
    pub graphs: Vec<&'a str>,
}

/// Bounded logical membership over the existing physical-owner catalog.
/// Ordinary Router/Owner admission still accepts only its existing single
/// graph contract. Consumers must qualify graph-set execution separately.
pub struct GraphSetCatalog {
    physical: std::sync::Arc<Catalog>,
    members: BTreeMap<String, String>,
    max_graphs: usize,
}

impl GraphSetCatalog {
    /// Validate the entire manifest before any file, queue or reader opens.
    /// Separate sets cannot share a file or lineage, even with identical pins.
    pub fn new(
        revision: u64,
        legacy_store_id: &str,
        sets: Vec<GraphSet>,
        budget: Budget,
        max_graphs: usize,
    ) -> Result<Self> {
        if max_graphs == 0 {
            return Err(refuse("graph-set membership bound is required"));
        }
        let mut members = BTreeMap::new();
        let mut routes = Vec::with_capacity(sets.len());
        for set in sets {
            if set.graphs.is_empty() || !set.graphs.contains(&set.owner.graph) {
                return Err(refuse("graph set must explicitly contain its owner graph"));
            }
            for graph in set.graphs {
                if !nonempty(&graph)
                    || graph == crate::schema::ROOT_GRAPH_IRI
                    || oxrdf::NamedNode::new(graph.as_str()).is_err()
                {
                    return Err(refuse("graph-set member must be an exact non-ROOT IRI"));
                }
                if members.insert(graph, set.owner.graph.clone()).is_some() {
                    return Err(refuse("logical graph has duplicate graph-set membership"));
                }
                if members.len() > max_graphs {
                    return Err(refuse("graph-set membership bound exceeded"));
                }
            }
            routes.push(set.owner);
        }
        let physical =
            std::sync::Arc::new(Catalog::new(revision, legacy_store_id, routes, budget)?);
        Ok(Self {
            physical,
            members,
            max_graphs,
        })
    }

    /// Physical reservations count writers, not the number of logical graphs.
    pub fn physical(&self) -> &Catalog {
        &self.physical
    }

    /// Explicit logical graphs remaining in the legacy file. ROOT must be
    /// selected by name too; this neither grants reads nor routes legacy writes.
    pub fn with_legacy_graphs(mut self, pin: Route, graphs: Vec<String>) -> Result<Self> {
        if !graphs.iter().any(|g| g == crate::schema::ROOT_GRAPH_IRI) {
            return Err(refuse("legacy graph set must explicitly contain ROOT"));
        }
        for graph in graphs {
            if !nonempty(&graph)
                || oxrdf::NamedNode::new(graph.as_str()).is_err()
                || self
                    .members
                    .insert(graph, crate::schema::ROOT_GRAPH_IRI.into())
                    .is_some()
            {
                return Err(refuse("invalid or duplicate legacy graph-set member"));
            }
            if self.members.len() > self.max_graphs {
                return Err(refuse("graph-set membership bound exceeded"));
            }
        }
        let physical = std::sync::Arc::try_unwrap(self.physical)
            .map_err(|_| refuse("legacy membership must be fixed before owner preparation"))?;
        self.physical = std::sync::Arc::new(physical.with_legacy_participant(pin)?);
        Ok(self)
    }

    #[cfg(unix)]
    pub(crate) fn logical_route(&self, graph: &str) -> Option<Route> {
        self.owner(graph).map(|owner| Route {
            graph: graph.into(),
            ..owner.clone()
        })
    }

    #[cfg(unix)]
    pub(crate) fn physical_pin(&self) -> std::sync::Arc<Catalog> {
        self.physical.clone()
    }

    /// Complete configured membership of a physical owner, never a read grant.
    pub fn owner_graphs(&self, graph: &str) -> Result<Vec<&str>> {
        let owner = self
            .owner(graph)
            .ok_or_else(|| refuse("unregistered graph-set owner"))?;
        Ok(self
            .members
            .iter()
            .filter(|(_, key)| **key == owner.graph)
            .map(|(member, _)| member.as_str())
            .collect())
    }

    fn owner(&self, graph: &str) -> Option<&Route> {
        self.members.get(graph).and_then(|key| {
            self.physical.routes.get(key).or_else(|| {
                self.physical
                    .legacy_participant()
                    .filter(|route| &route.graph == key)
            })
        })
    }

    /// Resolve a complete routed batch to ONE physical owner. Unknown members,
    /// legacy mixtures, stale pins and cross-file batches refuse before work.
    /// Signed replay and cross-file validation retain existing refusals.
    pub fn plan_batch(&self, requests: &[WriteRequest<'_>]) -> Result<OwnerSelection<'_>> {
        if requests.is_empty() || requests.len() > self.max_graphs {
            return Err(refuse("invalid graph-set batch size"));
        }
        let mut selected = BTreeMap::new();
        let mut owner = None;
        for request in requests {
            let graph = request
                .graph
                .ok_or_else(|| refuse("explicit graph-set target required"))?;
            let route = self
                .owner(graph)
                .ok_or_else(|| refuse("unregistered graph-set target"))?;
            if route.graph == crate::schema::ROOT_GRAPH_IRI {
                return Err(refuse("legacy writes retain their ordinary writer path"));
            }
            // Validate the same physical pins through the established contract;
            // the supplied logical graph is never reinterpreted as a path.
            self.physical.write_target(&WriteRequest {
                graph: Some(&route.graph),
                catalog_revision: request.catalog_revision,
                owner_generation: request.owner_generation,
                policy: request.policy,
                signed: request.signed,
                cross_graph_context: request.cross_graph_context,
            })?;
            if owner.is_some_and(|first: &Route| first != route) {
                return Err(refuse("multi-file write batch is unsupported"));
            }
            owner = Some(route);
            let member = self.members.get_key_value(graph).unwrap().0.as_str();
            if selected.insert(member, ()).is_some() {
                return Err(refuse("duplicate graph-set batch target"));
            }
        }
        Ok(OwnerSelection {
            owner: owner.unwrap(),
            graphs: selected.into_keys().collect(),
        })
    }

    /// Refuse any claim shared with another, separately configured catalog:
    /// a logical graph routed by both, or one physical file or store lineage
    /// named by both (canonical roots compared). The shared legacy store is
    /// the one legitimate overlap and is not a routed claim. An unreadable
    /// root refuses rather than assuming the files differ.
    pub fn refuse_shared_claims(
        &self,
        root: &std::path::Path,
        other: &Catalog,
        other_root: &std::path::Path,
    ) -> Result<()> {
        let same_root = root
            .canonicalize()
            .map_err(|_| refuse("graph-set root cannot be resolved"))?
            == other_root
                .canonicalize()
                .map_err(|_| refuse("other catalog root cannot be resolved"))?;
        for theirs in other.routes() {
            if self
                .owner(&theirs.graph)
                .is_some_and(|owner| owner.graph != crate::schema::ROOT_GRAPH_IRI)
            {
                return Err(refuse("a graph is claimed by two catalogs"));
            }
            for ours in self.physical.routes() {
                if ours.store_id == theirs.store_id || (same_root && ours.file_id == theirs.file_id)
                {
                    return Err(refuse("a physical owner file is claimed by two catalogs"));
                }
            }
        }
        Ok(())
    }

    /// Group an already-authorized explicit read dataset by physical owner.
    /// Members in one file need one snapshot/fence, not independent captures.
    /// Unknown/duplicate members refuse; no implicit union or partial plan.
    pub fn plan_read(&self, graphs: &[&str]) -> Result<Vec<OwnerSelection<'_>>> {
        if graphs.is_empty() || graphs.len() > self.max_graphs {
            return Err(refuse("invalid graph-set read size"));
        }
        let mut grouped: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
        for graph in graphs {
            let route = self
                .owner(graph)
                .ok_or_else(|| refuse("unregistered graph-set read"))?;
            let member = self.members.get_key_value(*graph).unwrap().0.as_str();
            if !grouped.entry(&route.graph).or_default().insert(member) {
                return Err(refuse("duplicate graph-set read member"));
            }
        }
        Ok(grouped
            .into_iter()
            .map(|(key, graphs)| OwnerSelection {
                owner: self.owner(key).unwrap(),
                graphs: graphs.into_iter().collect(),
            })
            .collect())
    }
}

mod file;
pub use file::GRAPH_SET_FILE_VERSION;

#[cfg(test)]
mod tests;
