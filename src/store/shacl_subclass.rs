//! Post-state SHACL context: the rdfs:subClassOf closure of touched types.
//! Split from `shacl_poststate.rs` to keep that file under the size gate.

use std::collections::BTreeSet;

use super::Store;
use crate::{Error, Result};

impl Store {
    pub(super) fn append_subclass_context(
        &self,
        facts: &mut Vec<crate::Fact>,
        graph: i64,
    ) -> Result<()> {
        let Some(rdf_type) = self.lookup("http://www.w3.org/1999/02/22-rdf-syntax-ns#type")? else {
            return Ok(());
        };
        let Some(subclass) = self.lookup("http://www.w3.org/2000/01/rdf-schema#subClassOf")? else {
            return Ok(());
        };
        let mut pending: Vec<_> = facts
            .iter()
            .filter_map(|f| match f.value {
                crate::Value::Ref(class) if f.attribute == rdf_type => Some(class),
                _ => None,
            })
            .collect();
        let mut seen = BTreeSet::new();
        while let Some(class) = pending.pop() {
            if !seen.insert(class) {
                continue;
            }
            if seen.len() > 1024 {
                return Err(Error::CannotVerify(
                    "post-state SHACL type hierarchy exceeds 1024 classes".into(),
                ));
            }
            let start = facts.len();
            for g in BTreeSet::from([0, graph]) {
                self.append_bounded_entity(facts, class, g, Some(subclass))?;
            }
            pending.extend(facts[start..].iter().filter_map(|f| match f.value {
                crate::Value::Ref(parent) => Some(parent),
                _ => None,
            }));
        }
        Ok(())
    }
}
