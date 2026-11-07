//! The source tag is provenance, never permission to forge mapping evidence.
//! Only the compiled lifecycle writer enters this private synchronous scope.

use std::cell::Cell;
use std::collections::BTreeSet;

use crate::{Datum, Error, Result, Store, Value};

thread_local! {
    static WRITER: Cell<bool> = const { Cell::new(false) };
}

pub(crate) fn with_writer<T>(work: impl FnOnce() -> T) -> T {
    struct Restore(bool);
    impl Drop for Restore {
        fn drop(&mut self) {
            WRITER.with(|slot| slot.set(self.0));
        }
    }
    let _restore = Restore(WRITER.with(|slot| slot.replace(true)));
    work()
}

fn protected_type(iri: &str) -> bool {
    PROTECTED
        .iter()
        .any(|name| iri == format!("{}{name}", super::facts::NS))
}

const PROTECTED: [&str; 5] = [
    "ShapeMapping",
    "ShapeVersionPin",
    "ShapeCorrespondence",
    "MappingRecord",
    "MappingDecision",
];

pub(crate) fn check(store: &Store, graph: i64, datums: &[Datum]) -> Result<()> {
    if WRITER.with(Cell::get) {
        return Ok(());
    }
    let rdf_type = store.lookup(crate::namespace::RDF_TYPE)?;
    let mut entities = BTreeSet::new();
    for datum in datums {
        let subject = store.resolve(datum.entity)?;
        let new_type = Some(datum.attribute) == rdf_type
            && match &datum.value {
                Value::Ref(id) => protected_type(&store.resolve(*id)?),
                _ => false,
            };
        if subject.starts_with("urn:quipu:mapping:") || new_type {
            return Err(Error::PolicyDenied(
                "mapping evidence requires its protected lifecycle writer".into(),
            ));
        }
        entities.insert(datum.entity);
    }
    let Some(rdf_type) = rdf_type else {
        return Ok(());
    };
    let kinds = PROTECTED
        .into_iter()
        .map(|name| {
            Ok(store
                .lookup(&format!("{}{name}", super::facts::NS))?
                .map(|id| Value::Ref(id).to_bytes()))
        })
        .collect::<Result<Vec<_>>>()?;
    if kinds.iter().all(Option::is_none) {
        return Ok(());
    }
    // Existence only, using the graph/entity/type index. Do not materialize a
    // subject's potentially large payload merely to check its protected kind.
    // Admission reserves the identity across retirement: a closed historical
    // type is still evidence that generic writers cannot reuse this subject.
    let mut statement = store.conn.prepare(
        "SELECT 1 FROM facts WHERE g=?1 AND e=?2 AND a=?3 AND op=1 AND v IN (?4,?5,?6,?7,?8) LIMIT 1"
    )?;
    for entity in entities {
        if statement.exists(rusqlite::params![
            graph, entity, rdf_type, kinds[0], kinds[1], kinds[2], kinds[3], kinds[4]
        ])? {
            return Err(Error::PolicyDenied(
                "existing mapping evidence requires its protected lifecycle writer".into(),
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Op;

    #[test]
    fn source_string_does_not_authorize_evidence_and_panic_restores_scope() {
        let mut store = Store::open_in_memory().unwrap();
        let datum = Datum {
            entity: store.intern("urn:fixture:mapping").unwrap(),
            attribute: store.intern(crate::namespace::RDF_TYPE).unwrap(),
            value: Value::Ref(
                store
                    .intern(&format!("{}MappingRecord", super::super::facts::NS))
                    .unwrap(),
            ),
            valid_from: "2026-10-25T00:00:00Z".into(),
            valid_to: None,
            op: Op::Assert,
        };
        assert!(
            store
                .transact(
                    std::slice::from_ref(&datum),
                    &datum.valid_from,
                    Some("actor"),
                    Some("quipu:shape-mapping")
                )
                .is_err()
        );
        assert!(with_writer(|| check(&store, 0, std::slice::from_ref(&datum))).is_ok());
        let panic = std::panic::catch_unwind(|| with_writer(|| panic!("fixture")));
        assert!(panic.is_err());
        assert!(check(&store, 0, &[datum]).is_err());
        assert!(check(&store, 0, &[]).is_ok());
    }
}
