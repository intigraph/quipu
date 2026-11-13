use super::*;
use crate::Store;
use crate::sparql::TemporalContext;
use oxrdf::Literal;
use spargebra::term::Variable;

fn variable(name: &str) -> Expression {
    Expression::Variable(Variable::new(name).unwrap())
}
fn literal(value: bool) -> Expression {
    Expression::Literal(Literal::from(value))
}
fn typed(value: &str, kind: &str) -> Value {
    Value::Typed {
        lexical: value.into(),
        datatype: format!("http://www.w3.org/2001/XMLSchema#{kind}"),
    }
}
fn values() -> Vec<Option<Value>> {
    vec![
        None,
        Some(Value::Bool(false)),
        Some(Value::Bool(true)),
        Some(Value::Int(0)),
        Some(Value::Int(i64::MAX)),
        Some(Value::Float(f64::NAN)),
        Some(Value::Float(f64::INFINITY)),
        Some(Value::Str(String::new())),
        Some(Value::Str("open".into())),
        Some(Value::Ref(1)),
        Some(Value::Bytes(vec![1])),
        Some(Value::Lang {
            lexical: "open".into(),
            lang: "en".into(),
        }),
        Some(typed("9007199254740993", "integer")),
        Some(typed("1.25", "decimal")),
        Some(typed("broken", "decimal")),
        Some(typed("2026-01-01T00:00:00Z", "dateTime")),
        Some(typed("2025-12-31T19:00:00-05:00", "dateTime")),
        Some(typed("2026-01-01T00:00:00", "dateTime")),
        Some(typed("broken", "dateTime")),
    ]
}
fn check(expressions: &[Expression]) {
    let store = Store::open_in_memory().unwrap();
    let ctx = TemporalContext::default();
    for expr in expressions {
        let filter = Program::filter(expr).expect("must exercise the compiled FILTER");
        let bind = Program::bind(expr).expect("must exercise the compiled BIND");
        for a in values() {
            for b in values() {
                let mut row = Bindings::default();
                if let Some(a) = &a {
                    row.insert("a".into(), a.clone());
                }
                if let Some(b) = b {
                    row.insert("b".into(), b);
                }
                assert_eq!(
                    filter.matches(&row),
                    filter::eval_filter(&store, expr, &row, &ctx).unwrap(),
                    "{expr:?} {row:?}"
                );
                assert_eq!(
                    bind.value(&row).map(|v| v.to_bytes()),
                    filter::eval_expr(&store, expr, &row).map(|v| v.to_bytes()),
                    "{expr:?} {row:?}"
                );
            }
        }
    }
}
#[test]
fn comparisons_preserve_typed_and_missing_values() {
    let a = Box::new(variable("a"));
    let b = Box::new(variable("b"));
    check(&[
        Expression::Equal(a.clone(), b.clone()),
        Expression::Greater(a.clone(), b.clone()),
        Expression::GreaterOrEqual(a.clone(), b.clone()),
        Expression::Less(a.clone(), b.clone()),
        Expression::LessOrEqual(a, b),
    ]);
}
#[test]
fn boolean_truth_tables_keep_filter_and_bind_distinct() {
    let a = Box::new(variable("a"));
    let b = Box::new(variable("b"));
    check(&[
        variable("a"),
        literal(true),
        Expression::Bound(Variable::new("a").unwrap()),
        Expression::Not(a.clone()),
        Expression::And(a.clone(), b.clone()),
        Expression::Or(a, b),
    ]);
    let row = Bindings::default();
    let expr = Expression::Or(Box::new(variable("a")), Box::new(literal(true)));
    assert!(Program::filter(&expr).unwrap().matches(&row));
    assert_eq!(Program::bind(&expr).unwrap().value(&row), None);
}
#[test]
fn lazy_if_coalesce_and_nested_boolean_bind_match() {
    let store = Store::open_in_memory().unwrap();
    let expr = Expression::If(
        Box::new(Expression::Bound(Variable::new("a").unwrap())),
        Box::new(Expression::Coalesce(vec![variable("a"), literal(false)])),
        Box::new(Expression::If(
            Box::new(literal(true)),
            Box::new(literal(true)),
            Box::new(variable("missing")),
        )),
    );
    let program = Program::bind(&expr).unwrap();
    for a in values() {
        let mut row = Bindings::default();
        if let Some(a) = a {
            row.insert("a".into(), a);
        }
        assert_eq!(program.value(&row), filter::eval_expr(&store, &expr, &row));
    }
    assert!(Program::filter(&expr).is_none());
}
#[test]
fn unsupported_subtrees_and_excess_slots_fall_back_whole() {
    use spargebra::algebra::Function;
    for expr in [
        Expression::FunctionCall(Function::Rand, vec![]),
        Expression::Add(Box::new(variable("a")), Box::new(variable("b"))),
    ] {
        assert!(Program::bind(&expr).is_none());
        assert!(Program::filter(&expr).is_none());
        let nested = Expression::If(
            Box::new(literal(false)),
            Box::new(expr),
            Box::new(literal(true)),
        );
        assert!(Program::bind(&nested).is_none());
    }
    let expr = Expression::Coalesce(
        (0..=MAX_SLOTS)
            .map(|i| variable(&format!("v{i}")))
            .collect(),
    );
    assert!(Program::bind(&expr).is_none());
    let repeated = Expression::Coalesce((0..100).map(|_| variable("a")).collect());
    assert_eq!(Program::bind(&repeated).unwrap().names, ["a"]);
}

/// Paired, local CPU control only; this is not a production stats benchmark.
#[test]
#[ignore = "bounded performance qualification; run explicitly with --ignored --nocapture"]
fn paired_expression_cpu_control() {
    use std::{hint::black_box, time::Instant};
    let store = Store::open_in_memory().unwrap();
    let expression = Expression::If(
        Box::new(Expression::And(
            Box::new(Expression::Bound(Variable::new("a").unwrap())),
            Box::new(Expression::Equal(
                Box::new(variable("a")),
                Box::new(Expression::Literal(Literal::new_simple_literal("open"))),
            )),
        )),
        Box::new(Expression::Literal(Literal::from(1))),
        Box::new(Expression::Literal(Literal::from(0))),
    );
    let compiled = Program::bind(&expression).unwrap();
    let rows: Vec<_> = ["open", "closed", "deferred"]
        .iter()
        .map(|status| {
            let mut row = Bindings::default();
            row.insert("a".into(), Value::Str((*status).into()));
            row
        })
        .collect();
    let mut timings = [Vec::new(), Vec::new()];
    for round in 0..6 {
        for mode in [round % 2, 1 - round % 2] {
            // Match the memo installed by count_stream::evaluate in both arms.
            let _memo = filter::EvalMemo::install();
            let started = Instant::now();
            let mut digest = 0_i64;
            for i in 0..616_000 {
                let row = black_box(&rows[i % rows.len()]);
                let value = if mode == 0 {
                    filter::eval_expr(&store, &expression, row)
                } else {
                    compiled.value(row)
                };
                let Some(Value::Int(n)) = black_box(value) else {
                    panic!("numeric result expected")
                };
                digest += n;
            }
            assert_eq!(digest, 205334);
            timings[mode].push(started.elapsed().as_secs_f64());
        }
    }
    println!(
        "paired local expression CPU seconds interpreter={:?} compiled={:?}; equal sum=205334, 616000 rows/run",
        timings[0], timings[1]
    );
}
