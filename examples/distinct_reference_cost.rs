use quipu::sparql::Bindings;
use quipu::sparql::aggregate::eval_aggregate;
use quipu::{Store, Value};
use spargebra::algebra::{AggregateExpression, AggregateFunction, Expression};
use spargebra::term::Variable;
use std::io::Write;
use std::time::Instant;

fn marker(phase: &str, start: Instant) {
    println!(
        "{{\"phase\":\"{phase}\",\"process_ms\":{}}}",
        start.elapsed().as_secs_f64() * 1000.0
    );
    std::io::stdout().flush().expect("flush phase evidence");
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let start = Instant::now();
    let case = std::env::args().nth(1).ok_or("one fixed case required")?;
    let (rows, expected, unbound, duplicates) = match case.as_str() {
        "positive" => (64, 64, false, false),
        "duplicates" => (128, 64, false, true),
        "unbound" => (64, 0, true, false),
        "distinct_refs" => (30_714, 30_714, false, false),
        _ => return Err("unsupported case; no arbitrary fixture size".into()),
    };
    marker("open", start);
    let store = Store::open_in_memory()?;
    marker("fixture", start);
    // Intermediate reference bindings exercise COUNT without dictionary lookup,
    // graph access, query parsing, or the separate streaming COUNT planner.
    let bindings: Vec<Bindings> = (0..rows)
        .map(|i| {
            if unbound {
                Bindings::new()
            } else {
                let id = if duplicates { i / 2 } else { i };
                [("s".to_string(), Value::Ref(id as i64 + 1))]
                    .into_iter()
                    .collect()
            }
        })
        .collect();
    let aggregate = AggregateExpression::FunctionCall {
        name: AggregateFunction::Count,
        expr: Expression::Variable(Variable::new("s")?),
        distinct: true,
    };
    marker("eval_start", start);
    let evaluator = Instant::now();
    let result = eval_aggregate(&store, &aggregate, &bindings);
    let elapsed_ms = evaluator.elapsed().as_secs_f64() * 1000.0;
    marker("eval_done", start);
    let Some(Value::Int(actual)) = result else {
        return Err("non-integer or missing aggregate result".into());
    };
    println!(
        "{{\"case\":\"{case}\",\"rows\":{rows},\"expected\":{expected},\"actual\":{actual},\"eval_ms\":{elapsed_ms}}}"
    );
    assert_eq!(actual, expected);
    Ok(())
}
