//! Graph context must retain stable identities, incoming edges and honest bounds.
use quipu::share_diff::{Snapshot, diff, neighborhood, parse_payload};

fn snapshot(data: &str) -> Snapshot {
    Snapshot::new(&parse_payload(data.as_bytes(), "test").unwrap())
}

#[test]
fn touched_entity_keeps_unchanged_incoming_context_and_graph_identity() {
    let common = "<http://e/b> <http://e/link> <http://e/a> <http://e/g> .\n";
    let old = snapshot(&format!("{common}<http://e/a> <http://e/value> \"old\" ."));
    let new = snapshot(&format!("{common}<http://e/a> <http://e/value> \"new\" ."));
    let context = neighborhood(&old, &new, &diff(&old, &new));
    assert_eq!(context.touched, ["<http://e/a>"]);
    assert_eq!(context.before.edges.len(), 2);
    assert_eq!(context.after.edges.len(), 2);
    let incoming = context
        .after
        .edges
        .iter()
        .find(|e| e.subject == "<http://e/b>")
        .unwrap();
    assert_eq!(incoming.object, "<http://e/a>");
    assert_eq!(incoming.graph, "<http://e/g>");
    assert!(!context.after.truncated);
}

#[test]
fn blank_relabel_only_has_no_touched_neighborhood() {
    let old = snapshot("<http://e/a> <http://e/link> _:old .\n_:old <http://e/p> \"same\" .\n");
    let new = snapshot("<http://e/a> <http://e/link> _:new .\n_:new <http://e/p> \"same\" .\n");
    let context = neighborhood(&old, &new, &diff(&old, &new));
    assert!(context.touched.is_empty());
    assert!(context.before.edges.is_empty());
    assert!(context.after.edges.is_empty());
}

#[test]
fn large_context_reports_its_bound_instead_of_hiding_omitted_edges() {
    let data: String = (0..2001)
        .map(|i| format!("<http://e/a> <http://e/p{i}> \"{i}\" .\n"))
        .collect();
    let (old, new) = (snapshot(""), snapshot(&data));
    let context = neighborhood(&old, &new, &diff(&old, &new));
    assert_eq!(context.after.total_edges, 2001);
    assert_eq!(context.after.edges.len(), 2000);
    assert!(context.after.truncated);
    assert_eq!(context.before.total_edges, 0);
}
