//! Live preconditions (decision-deck-seed-decisions.md §2). Read under the
//! deck savepoint with the same comparison as conditional writes, so a stale
//! card is refused before signing and again before admission, recording
//! nothing either time.

use super::*;
use crate::store::conditional::ExpectedValue;

/// The refusal for a changed, closed or missing item.
const STALE: &str = "stale request: the item changed";

impl Store {
    /// Check every expectation of `request` against current values. Terms are
    /// looked up, never interned: a slot whose terms do not exist holds no
    /// value, so it matches only an expectation of no value.
    pub(super) fn deck_check_expect(&mut self, request: &Request) -> Result<()> {
        let mut by_graph: std::collections::BTreeMap<&str, Vec<ExpectedValue>> =
            std::collections::BTreeMap::new();
        for e in &request.expect {
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
                Some(ExpectValue::Int(i)) => Some(Value::Int(*i)),
                Some(ExpectValue::Str(s)) => Some(Value::Str(s.clone())),
                Some(ExpectValue::Iri(iri)) => match self.lookup(iri)? {
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
