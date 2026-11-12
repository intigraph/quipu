//! Derived fluent registration with dependency-cycle checks.
use super::{Binding, Current, Log, Observation, Order, Policy, invalid, reduce};
use crate::Result;
use oxrdf::NamedNode;
use std::collections::{BTreeMap, BTreeSet};

/// A derived fluent, using the same deterministic projection policies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Observable {
    pub graph: String,
    pub name: String,
    pub observes: BTreeSet<String>,
    pub computed_by: Policy,
}

/// Observable definitions are separately governed configuration, not log events.
#[derive(Debug, Clone)]
pub struct Registry {
    graph: String,
    bases: BTreeMap<String, Binding>,
    definitions: BTreeMap<String, Observable>,
}

impl Registry {
    pub(crate) const BASE_LIMIT: usize = 128;
    pub(crate) const OBSERVABLE_LIMIT: usize = 128;

    /// Start one graph-scoped registry. This does not authorize Store writes.
    pub fn new(graph: &str) -> Result<Self> {
        NamedNode::new(graph).map_err(|_| invalid("observable graph must be an IRI"))?;
        Ok(Self {
            graph: graph.into(),
            bases: BTreeMap::new(),
            definitions: BTreeMap::new(),
        })
    }

    /// Retain a base's selected declaration without changing its interpretation.
    pub fn declare(&mut self, name: &str, binding: Binding) -> Result<()> {
        NamedNode::new(name).map_err(|_| invalid("fluent identity must be an IRI"))?;
        for iri in [
            &binding.subject,
            &binding.declaration.shape,
            &binding.declaration.predicate,
        ] {
            NamedNode::new(iri.as_str()).map_err(|_| invalid("binding identities must be IRIs"))?;
        }
        if binding.graph != self.graph || self.definitions.contains_key(name) {
            return Err(invalid(
                "foreign graph or observable identity used as a base",
            ));
        }
        if let Some(old) = self.bases.get(name) {
            return if old == &binding {
                Ok(())
            } else {
                Err(invalid("fluent declaration identity changed"))
            };
        }
        if self.bases.len() >= Self::BASE_LIMIT {
            return Err(invalid("base registry exceeds 128 declarations"));
        }
        self.bases.insert(name.into(), binding);
        Ok(())
    }

    /// Registration is atomic and rejects direct or transitive cycles.
    pub fn register(&mut self, observable: Observable) -> Result<()> {
        NamedNode::new(observable.name.as_str())
            .map_err(|_| invalid("observable identity must be an IRI"))?;
        if observable.graph != self.graph || self.bases.contains_key(&observable.name) {
            return Err(invalid("foreign graph or base identity used as observable"));
        }
        if let Some(old) = self.definitions.get(&observable.name) {
            return if old == &observable {
                Ok(())
            } else {
                Err(invalid("observable declaration identity changed"))
            };
        }
        for dependency in &observable.observes {
            if dependency != &observable.name
                && !self.bases.contains_key(dependency)
                && !self.definitions.contains_key(dependency)
            {
                return Err(invalid("unknown or cross-graph observable dependency"));
            }
        }
        if observable.name.is_empty()
            || observable.observes.is_empty()
            || observable.observes.iter().any(String::is_empty)
            || observable.observes.len() > 128
        {
            return Err(invalid(
                "observable needs identity and at most 128 dependencies",
            ));
        }
        let mut candidate = self.clone();
        candidate
            .definitions
            .insert(observable.name.clone(), observable);
        if candidate.definitions.len() > Self::OBSERVABLE_LIMIT {
            return Err(invalid("observable registry exceeds 128 definitions"));
        }
        let mut done = BTreeSet::new();
        for name in candidate.definitions.keys() {
            candidate.visit(name, &mut BTreeSet::new(), &mut done)?;
        }
        *self = candidate;
        Ok(())
    }

    fn visit(
        &self,
        name: &str,
        active: &mut BTreeSet<String>,
        done: &mut BTreeSet<String>,
    ) -> Result<()> {
        if done.contains(name) {
            return Ok(());
        }
        if !active.insert(name.to_owned()) {
            return Err(invalid("observable dependency cycle"));
        }
        if let Some(definition) = self.definitions.get(name) {
            for dependency in &definition.observes {
                self.visit(dependency, active, done)?;
            }
        }
        active.remove(name);
        done.insert(name.to_owned());
        Ok(())
    }

    /// Derive with the immutable policies declared in this graph registry.
    /// Missing inputs refuse rather than becoming zero or an empty set. Shared
    /// dependencies are memoized so a diamond graph does not multiply work.
    pub fn current(&self, name: &str, log: &Log) -> Result<Current> {
        self.evaluate(name, log, &mut BTreeMap::new(), &mut 1024)
    }

    fn evaluate(
        &self,
        name: &str,
        log: &Log,
        cache: &mut BTreeMap<String, Current>,
        remaining: &mut usize,
    ) -> Result<Current> {
        if let Some(current) = cache.get(name) {
            return Ok(current.clone());
        }
        *remaining = remaining
            .checked_sub(1)
            .ok_or_else(|| invalid("observable evaluation budget exceeded"))?;
        let Some(definition) = self.definitions.get(name) else {
            let binding = self
                .bases
                .get(name)
                .ok_or_else(|| invalid("undeclared input fluent"))?;
            let current = log
                .current(name, binding.declaration.policy)?
                .ok_or_else(|| invalid("observable input has no observations"))?;
            cache.insert(name.to_owned(), current.clone());
            return Ok(current);
        };
        let mut events = Vec::new();
        let mut provenance = BTreeSet::new();
        let mut latest: Option<(Order, Current)> = None;
        for dependency in &definition.observes {
            let current = self.evaluate(dependency, log, cache, remaining)?;
            let order = current
                .produced_by
                .iter()
                .filter_map(|id| log.events.get(id))
                .map(|o| &o.order)
                .max()
                .ok_or_else(|| invalid("observable source event missing"))?;
            if latest.as_ref().is_none_or(|(old, _)| old < order) {
                latest = Some((order.clone(), current.clone()));
            }
            provenance.extend(current.produced_by);
            for (index, value) in current.values.into_iter().enumerate() {
                events.push(Observation {
                    fluent: name.to_owned(),
                    order: Order {
                        tx: 1,
                        writer: dependency.clone(),
                        event: format!("{dependency}/{index}"),
                    },
                    as_of: "derived".into(),
                    recorded_on: "derived".into(),
                    value,
                });
            }
        }
        let current = if definition.computed_by == Policy::Latest {
            latest.ok_or_else(|| invalid("empty observable"))?.1
        } else {
            let refs: Vec<_> = events.iter().collect();
            let mut current = reduce(&refs, definition.computed_by)?
                .ok_or_else(|| invalid("empty observable"))?;
            current.produced_by = provenance;
            current
        };
        cache.insert(name.to_owned(), current.clone());
        Ok(current)
    }
}
