//! Immutable observable configuration through the native transaction gates.
use super::{Current, Observable, invalid, native_registry, writer};
use crate::store::Datum;
use crate::{Result, Store};

#[derive(Debug, Clone)]
pub(crate) struct Register {
    pub observable: Observable,
    pub subject: String,
    pub predicate: String,
    pub property_shape: String,
}

impl Register {
    pub(super) fn materialize(
        &self,
        store: &Store,
        graph: i64,
        _tx: i64,
        timestamp: &str,
    ) -> Result<writer::Prepared> {
        let identity = crate::transaction_auth::current_identity()
            .ok_or_else(|| invalid("native registration requires authenticated writer evidence"))?;
        writer::iri(&identity.principal)?;
        writer::iri(&self.observable.name)?;
        let binding = writer::binding_for(
            store,
            graph,
            &self.subject,
            &self.predicate,
            &self.property_shape,
        )?;
        if self.observable.graph != binding.graph
            || self.observable.computed_by != binding.declaration.policy
        {
            return Err(invalid(
                "observable graph or computation does not match property shape",
            ));
        }
        store.check_fluent_graph_binding(&self.observable.name, &binding.graph)?;
        let (mut registry, log) = native_registry::load(store, graph)?;
        registry.register(self.observable.clone())?;
        // Missing inputs, numeric incompatibility and overflow refuse before
        // admitting a configuration that cannot compute a current result.
        registry.current(&self.observable.name, &log)?;
        let mut datums: Vec<Datum> =
            writer::descriptor(store, &binding, &self.observable.name, timestamp)?;
        let qp = crate::namespace::QUIPU;
        datums.push(writer::datum(
            store,
            &self.observable.name,
            crate::namespace::RDF_TYPE,
            &writer::iri(&format!("{qp}Observable"))?,
            timestamp,
        )?);
        datums.push(writer::datum(
            store,
            &self.observable.name,
            &format!("{qp}computedBy"),
            &writer::literal(writer::policy_name(self.observable.computed_by)),
            timestamp,
        )?);
        for dependency in &self.observable.observes {
            datums.push(writer::datum(
                store,
                &self.observable.name,
                &format!("{qp}observes"),
                &writer::iri(dependency)?,
                timestamp,
            )?);
        }
        if let Some(entity) = store.lookup(&self.observable.name)? {
            let prior = store.entity_facts_in_graph(entity, graph)?;
            if !prior.is_empty() {
                if prior.len() != datums.len()
                    || datums.iter().any(|d| {
                        !prior
                            .iter()
                            .any(|f| f.attribute == d.attribute && f.value == d.value)
                    })
                {
                    return Err(invalid("native observable declaration changed"));
                }
                return Ok(writer::Prepared::Replay(prior[0].tx));
            }
        }
        Ok(writer::Prepared::Write(datums))
    }
}

impl Store {
    /// Internal graph-scoped reader; no HTTP/CLI read grant is implied.
    #[allow(dead_code)]
    pub(crate) fn fluent_current_native(&self, graph: i64, name: &str) -> Result<Current> {
        let (registry, log) = native_registry::load(self, graph)?;
        registry.current(name, &log)
    }
}

#[cfg(all(test, feature = "shacl"))]
#[path = "registration_tests.rs"]
mod tests;
