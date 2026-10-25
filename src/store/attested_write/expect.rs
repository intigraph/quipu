//! Live preconditions, read under the commit savepoint with the same
//! comparison as conditional writes. Salvaged from the closed Deck
//! precondition work (quipu #815) under domain-free names.

use super::*;
use crate::store::conditional::ExpectedValue;

/// The refusal for any precondition that no longer holds.
const STALE: &str = "a precondition no longer holds";

pub(super) fn valid_expect(expect: &[Expect]) -> bool {
    let slots: std::collections::BTreeSet<_> =
        expect.iter().map(|e| (&e.graph, &e.s, &e.p)).collect();
    expect.len() <= MAX_EXPECT
        && slots.len() == expect.len()
        && expect.iter().all(|e| {
            iri(&e.graph) && iri(&e.s) && iri(&e.p) && e.o.as_ref().is_none_or(valid_value)
        })
}

impl Store {
    /// Check every expectation against current values. Terms are looked up,
    /// never interned: a slot whose terms do not exist holds no value, so it
    /// matches only an expectation of no value. An unregistered graph is
    /// refused, never read as empty.
    pub(super) fn attest_check_expect(&mut self, expect: &[Expect]) -> Result<()> {
        let mut by_graph: std::collections::BTreeMap<&str, Vec<ExpectedValue>> =
            std::collections::BTreeMap::new();
        for e in expect {
            if self.registered_graph_id(&e.graph)?.is_none() {
                return Err(refuse(STALE));
            }
            let (Some(entity), Some(attribute)) = (self.lookup(&e.s)?, self.lookup(&e.p)?) else {
                if e.o.is_some() {
                    return Err(refuse(STALE));
                }
                continue;
            };
            let value = match &e.o {
                None => None,
                Some(TypedValue::Int(i)) => Some(Value::Int(*i)),
                Some(TypedValue::Str(s)) => Some(Value::Str(s.clone())),
                Some(TypedValue::Iri(iri)) => match self.lookup(iri)? {
                    Some(id) => Some(Value::Ref(id)),
                    None => return Err(refuse(STALE)),
                },
            };
            by_graph.entry(&e.graph).or_default().push(ExpectedValue {
                entity,
                attribute,
                value,
            });
        }
        for (graph, expected) in by_graph {
            let graph = self
                .registered_graph_id(graph)?
                .ok_or_else(|| refuse(STALE))?;
            match self.check_expected(graph, &expected, &[], None) {
                Err(Error::ConditionalWriteConflict) => return Err(refuse(STALE)),
                other => other?,
            }
        }
        Ok(())
    }
}
