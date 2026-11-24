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
    physical: Catalog,
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
        let physical = Catalog::new(revision, legacy_store_id, routes, budget)?;
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

    fn owner(&self, graph: &str) -> Option<&Route> {
        self.members
            .get(graph)
            .and_then(|key| self.physical.routes.get(key))
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
                owner: &self.physical.routes[key],
                graphs: graphs.into_iter().collect(),
            })
            .collect())
    }
}

#[cfg(test)]
mod tests;
