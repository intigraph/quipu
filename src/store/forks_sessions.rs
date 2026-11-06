//! Native profiled fork adapter helpers; no runtime activation.
use super::*;

impl Store {
    /// Common report rows, including agreeing changes. No writes.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn fork_review_records(
        &self,
        name: &str,
    ) -> Result<(ForkInfo, Vec<crate::merge3::DecisionRecord>)> {
        let fork = self.require_open_fork(name, "inspect")?;
        let (b, o, t) = self.fork_sides(&fork)?;
        let graph = |facts: Vec<Fact>| -> Result<crate::share_merge::Graph> {
            let mut out = crate::share_merge::Graph::new();
            for f in facts {
                let subject = crate::rdf::value_to_term(self, &crate::types::Value::Ref(f.entity))?;
                let predicate = self.resolve(f.attribute)?;
                let value = crate::rdf::value_to_term(self, &f.value)?;
                out.extend(crate::share_merge::parse_graph(
                    &format!("{subject} <{predicate}> {value} ."),
                    "fork review",
                )?);
            }
            Ok(out)
        };
        let shapes = self.get_combined_shapes()?.unwrap_or_default();
        let (_, rows) = crate::merge_review::merge(&graph(b)?, &graph(o)?, &graph(t)?, &shapes)?;
        Ok((fork, rows))
    }

    /// Snapshot a fork for the common session planner.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn fork_session_snapshot(
        &self,
        name: &str,
    ) -> Result<(ForkInfo, String, String, String)> {
        let fork = self
            .fork_lookup(name)?
            .ok_or_else(|| Error::InvalidValue("unknown fork".into()))?;
        let (base, ours, theirs) = self.fork_sides(&fork)?;
        let nt = |facts: Vec<Fact>| -> Result<String> {
            let bytes = crate::rdf::serialize_facts(self, &facts, oxrdfio::RdfFormat::NTriples)?;
            let text = String::from_utf8(bytes).map_err(|e| Error::Serialization(e.to_string()))?;
            Ok(crate::merge_session::render(
                &crate::share_merge::parse_graph(&text, "fork session")?,
            ))
        };
        Ok((fork, nt(base)?, nt(ours)?, nt(theirs)?))
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn finish_fork_session(
        &mut self,
        name: &str,
        id: &str,
        timestamp: &str,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE forks SET status='promoted' WHERE name=?1 AND status='open'",
            params![name],
        )?;
        self.emit_registry_event_with(
            "fork.promoted",
            name,
            timestamp,
            serde_json::json!({"merge_id":id}),
        )
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn fork_session_closes(
        &self,
        name: &str,
        slot: &crate::merge_lists::Slot,
    ) -> Result<std::collections::BTreeMap<String, String>> {
        let fork = self
            .fork_lookup(name)?
            .ok_or_else(|| Error::InvalidValue("unknown fork".into()))?;
        let subject = slot
            .0
            .strip_prefix('<')
            .and_then(|s| s.strip_suffix('>'))
            .ok_or_else(|| Error::InvalidValue("stable fork subject required".into()))?;
        let (Some(e), Some(a)) = (self.lookup(subject)?, self.lookup(&slot.1)?) else {
            return Ok(Default::default());
        };
        let closed = self.fork_closes(fork.g, e, a)?;
        let mut out = std::collections::BTreeMap::new();
        for fact in self.root_facts_as_of(fork.fork_tx)? {
            if fact.entity == e
                && fact.attribute == a
                && let Some(at) = closed.get(&fact.value.term_key())
            {
                out.insert(
                    crate::rdf::value_to_term(self, &fact.value)?.to_string(),
                    at.clone(),
                );
            }
        }
        Ok(out)
    }
}
