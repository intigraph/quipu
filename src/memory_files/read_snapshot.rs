//! Internal authorized-view copying, never a write capability for a caller's Store.
use crate::{Result, Store};

/// Build a fresh, request-owned scratch store from bounded authorized graph rows.
/// The caller has already pinned and filtered the source snapshot. No destination,
/// callback or permit is accepted or returned, and the ordinary write guard stays on.
pub(crate) fn materialize(graphs: &[(String, String)]) -> Result<(Store, Vec<i64>)> {
    let mut scratch = Store::open_in_memory()?;
    let mut ids = Vec::new();
    for (iri, canonical) in graphs {
        let graph = if iri == crate::schema::ROOT_GRAPH_IRI {
            0
        } else {
            scratch.graph_create(iri)?
        };
        let rows = crate::rdf::parse_rdf(
            &scratch,
            canonical.as_bytes(),
            oxrdfio::RdfFormat::NTriples,
            None,
            "1970-01-01T00:00:00Z",
        )?;
        // Exact remapped rows, only in the store created above. RAII drops the
        // permit on success/error; no external work runs inside its lifetime.
        super::write_scope::protocol(graph, &rows, || {
            scratch.transact_to_graph(&rows, "1970-01-01T00:00:00Z", None, None, graph)
        })?;
        ids.push(graph);
    }
    Ok((scratch, ids))
}

#[cfg(test)]
mod tests {
    use super::*;

    const RESERVED: &str =
        "<urn:quipu:memory:owner:copy> <https://quipu.dev/memory#head> \"snapshot\" .";

    fn generic(store: &mut Store, graph: i64) -> crate::Result<(i64, usize)> {
        crate::rdf::ingest_assigned_rdf(
            store,
            RESERVED.as_bytes(),
            oxrdfio::RdfFormat::NTriples,
            None,
            "1970-01-01T00:00:00Z",
            None,
            None,
            graph,
        )
    }

    #[test]
    fn reserved_copy_is_fresh_scratch_only_and_does_not_leak_a_write_permit() {
        let mut outside = Store::open_in_memory().unwrap();
        let graph = outside.graph_create("urn:memory").unwrap();
        assert!(matches!(
            generic(&mut outside, graph),
            Err(crate::Error::PolicyDenied(_))
        ));
        let (mut scratch, ids) = materialize(&[("urn:memory".into(), RESERVED.into())]).unwrap();
        assert_eq!(ids.len(), 1);
        // Both stores deliberately have the same graph allocation: the permit
        // cannot survive the copy even where an outside graph id would match.
        assert_eq!(ids[0], graph);
        assert!(matches!(
            generic(&mut outside, graph),
            Err(crate::Error::PolicyDenied(_))
        ));
        assert!(matches!(
            generic(&mut scratch, ids[0]),
            Err(crate::Error::PolicyDenied(_))
        ));
    }

    #[test]
    fn failed_scratch_copy_does_not_leave_a_permit_for_an_outside_store() {
        assert!(materialize(&[("urn:memory".into(), "not RDF".into())]).is_err());
        let mut outside = Store::open_in_memory().unwrap();
        let graph = outside.graph_create("urn:memory").unwrap();
        assert!(matches!(
            generic(&mut outside, graph),
            Err(crate::Error::PolicyDenied(_))
        ));
    }
}
