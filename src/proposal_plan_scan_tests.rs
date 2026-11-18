//! The named-graph scan's cost (aegis-pgc).
//!
//! On the production store, `GRAPH ?g { ?s a <Document> }` could not finish
//! inside the 30 s query budget: the class has many instances in ROOT, and the
//! whole-store pattern visits every one of them only to discard ROOT. So a
//! shape proposal naming a common class planned as `undetermined` and its
//! ordinary accept was refused. The scan answers "which named graphs use this
//! term", so its cost must follow the number of graphs, not the number of
//! instances outside them.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use super::{Terms, graphs_using};
use crate::store::Store;

const DOC: &str = "http://example.org/Doc";
const LIVE: &str = "http://example.org/g/live";

fn ingest(store: &mut Store, turtle: &str, graph: i64) {
    crate::rdf::ingest_rdf_chunked(
        store,
        turtle.as_bytes(),
        oxrdfio::RdfFormat::Turtle,
        None,
        "2026-01-01T00:00:00Z",
        Some("test"),
        Some("seed"),
        graph,
        10_000,
    )
    .unwrap();
}

/// A store with `root` instances of `DOC` in ROOT and one in a named graph.
fn store_with_root_docs(root: usize) -> Store {
    let mut store = Store::open_in_memory().unwrap();
    // No deadline: the step counter below owns the progress handler.
    store.search_config_mut().query_timeout_ms = 0;
    let ttl: String = (0..root)
        .map(|i| format!("<http://example.org/d{i}> a <{DOC}> .\n"))
        .collect();
    ingest(&mut store, &ttl, 0);
    let live = store.graph_create(LIVE).unwrap();
    ingest(
        &mut store,
        &format!("<http://example.org/x> a <{DOC}> ."),
        live,
    );
    store
}

fn class_terms() -> Terms {
    Terms {
        classes: BTreeSet::from([DOC.to_owned()]),
        ..Terms::default()
    }
}

/// `SQLite` VM progress callbacks spent by one named-graph scan.
fn scan_steps(store: &Store) -> (u64, Vec<String>) {
    let steps = Arc::new(AtomicU64::new(0));
    let counter = Arc::clone(&steps);
    store.conn.progress_handler(
        64,
        Some(move || {
            counter.fetch_add(1, Ordering::Relaxed);
            false
        }),
    );
    let result = graphs_using(store, &class_terms(), &BTreeSet::new());
    store.conn.progress_handler(0, None::<fn() -> bool>);
    let (graphs, snapshots) = result.unwrap();
    assert!(snapshots.is_empty(), "{snapshots:?}");
    (steps.load(Ordering::Relaxed), graphs)
}

#[test]
fn the_named_graph_scan_finds_the_graph_and_never_root() {
    let (_, graphs) = scan_steps(&store_with_root_docs(10));
    assert_eq!(graphs, [LIVE]);
}

/// Failing-first: the whole-store pattern grows with ROOT's instances.
#[test]
fn the_named_graph_scan_cost_does_not_grow_with_root_instances() {
    let (small, small_graphs) = scan_steps(&store_with_root_docs(50));
    let (large, large_graphs) = scan_steps(&store_with_root_docs(5_000));
    assert_eq!(small_graphs, [LIVE]);
    assert_eq!(large_graphs, [LIVE]);
    // 100x the ROOT instances may not cost even 4x the scan work.
    assert!(
        large <= small.max(1) * 4,
        "scan work grew with ROOT: {small} steps at 50 instances, {large} at 5000"
    );
}
