//! Opt-in, bounded actual execution evidence. No query text, IDs or RDF values.
//! Timings are inclusive and overlap; they must never be added as wall time.
use std::{cell::RefCell, collections::BTreeMap, time::Instant};

use serde_json::{Value, json};
use spargebra::algebra::GraphPattern;

use crate::{Error, error::Result};

const MAX_CHOICES: usize = 64;
#[derive(Default)]
struct State {
    operators: BTreeMap<&'static str, Totals>,
    choices: Vec<Value>,
    paths: BTreeMap<&'static str, (u64, u64, u64, u64)>,
    dropped: u64,
}
#[derive(Default)]
struct Totals {
    calls: u64,
    errors: u64,
    rows: u64,
    micros: u128,
}
thread_local! {
    static ACTIVE: RefCell<Option<State>> = const { RefCell::new(None) };
}
struct Scope(Option<State>);
impl Drop for Scope {
    fn drop(&mut self) {
        ACTIVE.with(|active| *active.borrow_mut() = self.0.take());
    }
}

pub(crate) fn run(input: &Value, work: impl FnOnce() -> Result<Value>) -> Result<Value> {
    let enabled = match input.get("execution_profile") {
        None | Some(Value::Bool(false)) => false,
        Some(Value::Bool(true)) => true,
        Some(_) => {
            return Err(Error::InvalidValue(
                "execution_profile must be boolean".into(),
            ));
        }
    };
    if !enabled {
        return work();
    }
    if input.get("federated").and_then(Value::as_bool) == Some(true)
        || input.get("_sparql_protocol").and_then(Value::as_bool) == Some(true)
    {
        return Err(Error::InvalidValue(
            "execution_profile requires local bespoke JSON".into(),
        ));
    }
    let _scope = Scope(ACTIVE.with(|active| active.replace(Some(State::default()))));
    let started = Instant::now();
    let mut result = work()?;
    let state = ACTIVE.with(|active| active.borrow_mut().take().expect("profile scope"));
    let operators: Vec<_> = state
        .operators
        .into_iter()
        .map(|(name, t)| {
            json!({"operator":name,"calls":t.calls,"errors":t.errors,
            "output_rows_sum":t.rows,"inclusive_us":t.micros})
        })
        .collect();
    let paths: Vec<_> = state
        .paths
        .into_iter()
        .map(|(path, (calls, inputs, outputs, known))| {
            json!({"path":path,"calls":calls,"input_rows_sum":inputs,
            "output_rows_sum":outputs,"calls_with_output":known})
        })
        .collect();
    result["execution_profile"] = json!({
        "contract":"quipu-execution-profile/v1",
        "scope":"local tool_query including inference and label reads",
        "timing":"inclusive; overlapping operators are not additive",
        "elapsed_us":started.elapsed().as_micros(),
        "operators":operators,"paths":paths,"choices":state.choices,
        "choices_dropped":state.dropped,"choices_complete":state.dropped == 0,
        "coverage":"algebra operators, BGP dispatch and filter_join; not SQL EXPLAIN or all optimizer decisions"
    });
    Ok(result)
}

pub(super) fn begin() -> Option<Instant> {
    ACTIVE.with(|active| active.borrow().as_ref().map(|_| Instant::now()))
}

pub(super) fn choice(path: &'static str, input_rows: usize, output_rows: Option<usize>) {
    ACTIVE.with(|active| {
        let mut active = active.borrow_mut();
        let Some(state) = active.as_mut() else {
            return;
        };
        // Call sites use a fixed internal vocabulary. Aggregate counts survive
        // sample saturation, so many small BGPs cannot hide a later guard.
        let total = state.paths.entry(path).or_default();
        total.0 = total.0.saturating_add(1);
        total.1 = total.1.saturating_add(input_rows as u64);
        total.2 = total.2.saturating_add(output_rows.unwrap_or(0) as u64);
        total.3 = total.3.saturating_add(u64::from(output_rows.is_some()));
        if state.choices.len() == MAX_CHOICES {
            state.dropped = state.dropped.saturating_add(1);
        } else {
            state
                .choices
                .push(json!({"path":path,"input_rows":input_rows,"output_rows":output_rows}));
        }
    });
}

pub(super) fn finish(pattern: &GraphPattern, started: Option<Instant>, rows: Option<usize>) {
    let Some(started) = started else {
        return;
    };
    let name = match pattern {
        GraphPattern::Bgp { .. } => "bgp",
        GraphPattern::Join { .. } => "join",
        GraphPattern::Filter { .. } => "filter",
        GraphPattern::Union { .. } => "union",
        GraphPattern::Graph { .. } => "graph",
        GraphPattern::Project { .. } => "project",
        GraphPattern::Distinct { .. } => "distinct",
        GraphPattern::Slice { .. } => "slice",
        _ => "other",
    };
    ACTIVE.with(|active| {
        if let Some(state) = active.borrow_mut().as_mut() {
            let total = state.operators.entry(name).or_default();
            total.calls = total.calls.saturating_add(1);
            total.errors = total.errors.saturating_add(u64::from(rows.is_none()));
            total.rows = total.rows.saturating_add(rows.unwrap_or(0) as u64);
            total.micros = total.micros.saturating_add(started.elapsed().as_micros());
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_has_no_clock_or_profile() {
        assert!(begin().is_none());
        let answer = json!({"count":0});
        assert_eq!(run(&json!({}), || Ok(answer.clone())).unwrap(), answer);
        assert!(begin().is_none());
    }

    #[test]
    fn choices_are_bounded_and_loss_is_explicit() {
        let result = run(&json!({"execution_profile":true}), || {
            for _ in 0..100 {
                choice("fixture", 17, Some(0));
            }
            Ok(json!({"count":0}))
        })
        .unwrap();
        let p = &result["execution_profile"];
        assert_eq!(p["choices"].as_array().unwrap().len(), MAX_CHOICES);
        assert_eq!(p["paths"][0]["calls"], 100);
        assert_eq!(p["paths"][0]["input_rows_sum"], 1700);
        assert_eq!(p["paths"][0]["calls_with_output"], 100);
        assert_eq!(p["choices_dropped"], 36);
        assert_eq!(p["choices_complete"], false);
        assert_eq!(p["choices"][0]["input_rows"], 17);
        assert_eq!(p["choices"][0]["output_rows"], 0);
        assert!(begin().is_none());
    }

    #[test]
    fn error_and_panic_restore_scope() {
        assert!(
            run(&json!({"execution_profile":true}), || Err(
                Error::InvalidValue("fixture".into())
            ))
            .is_err()
        );
        assert!(begin().is_none());
        let _ = std::panic::catch_unwind(|| {
            run(&json!({"execution_profile":true}), || panic!("fixture"))
        });
        assert!(begin().is_none());
    }

    #[test]
    fn nested_scope_is_isolated_and_restored() {
        let outer = run(&json!({"execution_profile":true}), || {
            choice("outer", 1, Some(1));
            let child = run(&json!({"execution_profile":true}), || {
                choice("child", 2, Some(2));
                Ok(json!({}))
            })?;
            assert_eq!(child["execution_profile"]["choices"][0]["path"], "child");
            Ok(json!({}))
        })
        .unwrap();
        assert_eq!(
            outer["execution_profile"]["choices"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(outer["execution_profile"]["choices"][0]["path"], "outer");
    }
}
