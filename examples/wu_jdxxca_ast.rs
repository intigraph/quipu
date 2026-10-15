use spargebra::algebra::GraphPattern;
use spargebra::{Query, SparqlParser};
fn branches(p: &GraphPattern) -> Vec<GraphPattern> {
    match p {
        GraphPattern::Union { left, right } => {
            let mut out = branches(left);
            out.extend(branches(right));
            out
        }
        _ => vec![p.clone()],
    }
}
fn split(p: &mut GraphPattern, index: usize) -> usize {
    match p {
        GraphPattern::Slice { inner, .. }
        | GraphPattern::Project { inner, .. }
        | GraphPattern::OrderBy { inner, .. }
        | GraphPattern::Filter { inner, .. }
        | GraphPattern::Graph { inner, .. } => split(inner, index),
        GraphPattern::Union { .. } => {
            let all = branches(p);
            let count = all.len();
            if let Some(branch) = all.get(index) {
                *p = branch.clone();
            }
            count
        }
        _ => 0,
    }
}
fn main() {
    let path = std::env::args().nth(1).expect("query JSON path");
    let value: serde_json::Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let original = SparqlParser::new()
        .parse_query(value["query"].as_str().unwrap())
        .unwrap();
    for i in 0..32 {
        let mut query = original.clone();
        let Query::Select { pattern, .. } = &mut query else {
            panic!("SELECT required")
        };
        let count = split(pattern, i);
        if i >= count {
            break;
        }
        println!(
            "{}",
            serde_json::json!({"branch":i,"query":query.to_string()})
        );
    }
}
