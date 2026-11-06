//! Question verification over explicit typed observations.
//!
//! Adapters own store/network access, deadlines and response decoding. A timeout,
//! HTTP 408, partial result or unavailable decoder must return `Unknown`.
use std::collections::BTreeSet;
use std::time::Instant;

use super::report::{Block, Checks, Report, Status};
use super::{Contract, Question, lint};
use crate::share_queries::SharedQuery;
use crate::sparql::{QueryResult, Triple};
use crate::store::queries::{StoredParam, StoredQuery};
use crate::types::Value;

/// A transport failure is not negative query evidence or an empty result.
#[derive(Debug)]
pub enum Observation<T> {
    /// A complete, interpreted observation.
    Known(T),
    /// An established execution refusal or contract failure.
    Failed(String),
    /// Timeout, incomplete result, unavailable endpoint or missing evidence.
    Unknown(String),
}

/// Read-only evidence boundary, shared by local and remote verification.
pub trait Reader {
    /// Evaluate a rendered read query under the adapter's bounded deadline.
    fn query(&mut self, sparql: &str) -> Observation<QueryResult>;
    /// Establish existence of every declared justification target.
    fn targets_exist(&mut self, targets: &[String]) -> Observation<bool>;
    /// Validate graph results against the declared shape without staging them.
    fn graph_matches(&mut self, shape: &str, triples: &[Triple]) -> Observation<bool>;
    /// Resolve dictionary references as IRIs, distinguishing blank nodes.
    fn is_iri(&mut self, term: i64) -> Observation<bool>;
}

fn rendered(question: &Question, registry: &[SharedQuery]) -> crate::Result<String> {
    StoredQuery {
        name: question.id.clone(),
        description: question.ask.clone(),
        template: lint::query_text(question, registry)?,
        dataset: None,
        params: question
            .inputs
            .iter()
            .map(|(name, kind)| StoredParam {
                name: name.clone(),
                kind: kind.trim_end_matches('?').into(),
                required: !kind.ends_with('?'),
                default: None,
                description: String::new(),
            })
            .collect(),
    }
    .render(&question.example)
}

fn scalar(reader: &mut impl Reader, kind: &str, value: &Value) -> Option<bool> {
    const XSD: &str = "http://www.w3.org/2001/XMLSchema#";
    Some(match (kind, value) {
        ("iri", Value::Ref(term)) => return flag(reader.is_iri(*term)).0,
        ("string", Value::Str(_))
        | ("int" | "integer" | "decimal", Value::Int(_))
        | ("boolean", Value::Bool(_)) => true,
        ("string" | "decimal" | "date" | "dateTime", Value::Typed { datatype, .. }) => {
            datatype == &format!("{XSD}{kind}")
        }
        _ => false,
    })
}

fn flag(value: Observation<bool>) -> (Option<bool>, Option<String>) {
    match value {
        Observation::Known(value) => (Some(value), None),
        Observation::Failed(reason) => (Some(false), Some(reason)),
        Observation::Unknown(reason) => (None, Some(reason)),
    }
}

fn result_matches(
    reader: &mut impl Reader,
    question: &Question,
    result: &QueryResult,
) -> (Option<bool>, Option<String>) {
    match result {
        QueryResult::Ask(_) => (
            Some(question.returns.is_empty() && question.returns_shape.is_none()),
            None,
        ),
        QueryResult::Graph(triples) => match &question.returns_shape {
            Some(shape) if question.returns.is_empty() => {
                flag(reader.graph_matches(shape, triples))
            }
            _ => (Some(false), Some("unexpected graph result".into())),
        },
        QueryResult::Select { variables, rows } => {
            let columns: BTreeSet<_> = variables.iter().map(String::as_str).collect();
            if columns.len() != variables.len()
                || columns != question.returns.keys().map(String::as_str).collect()
                || question.returns.is_empty()
                || question.returns_shape.is_some()
            {
                return (
                    Some(false),
                    Some("result columns differ from returns".into()),
                );
            }
            let mut unknown = false;
            for row in rows {
                if row.keys().any(|name| !question.returns.contains_key(name)) {
                    return (Some(false), Some("unexpected result binding".into()));
                }
                for (name, kind) in &question.returns {
                    match row.get(name).and_then(|value| scalar(reader, kind, value)) {
                        Some(true) => {}
                        Some(false) => {
                            return (Some(false), Some("result scalar type differs".into()));
                        }
                        None if !row.contains_key(name) => {
                            return (
                                Some(false),
                                Some("required result binding is absent".into()),
                            );
                        }
                        None => unknown = true,
                    }
                }
            }
            if unknown {
                (None, Some("result term identity is unproved".into()))
            } else {
                (Some(true), None)
            }
        }
    }
}

fn expectation(expect: &str, result: &QueryResult, rows: u64) -> bool {
    match expect {
        "ask=true" => matches!(result, QueryResult::Ask(true)),
        "ask=false" => matches!(result, QueryResult::Ask(false)),
        "empty" => rows == 0,
        "nonempty" => rows > 0,
        other => {
            other
                .strip_prefix("rows=")
                .and_then(|n| n.parse::<u64>().ok())
                == Some(rows)
        }
    }
}

/// Verify one active question without installing its definition or running commands.
/// Callers validate the document schema and block identities before this function.
#[must_use]
pub fn question(reader: &mut impl Reader, question: &Question, registry: &[SharedQuery]) -> Block {
    let start = Instant::now();
    let mut checks = Checks::default();
    let mut rows = None;
    let reason = if let Err(error) = lint::question(question, registry) {
        checks.parses = Some(false);
        Some(error.to_string())
    } else if question.retired {
        checks.parses = Some(true);
        Some("retired question was not executed".into())
    } else {
        checks.parses = Some(true);
        match rendered(question, registry) {
            Err(_) => Some("example inputs could not be rendered; no query was run".into()),
            Ok(sparql) => match reader.query(&sparql) {
                Observation::Unknown(reason) => Some(reason),
                Observation::Failed(reason) => {
                    checks.runs = Some(false);
                    Some(reason)
                }
                Observation::Known(result) => {
                    checks.runs = Some(true);
                    let count = match &result {
                        QueryResult::Ask(value) => u64::from(*value),
                        QueryResult::Select { rows, .. } => rows.len() as u64,
                        QueryResult::Graph(triples) => triples.len() as u64,
                    };
                    rows = Some(count);
                    let (matches, mismatch) = result_matches(reader, question, &result);
                    checks.returns_match = matches;
                    checks.expect = question
                        .expect
                        .as_deref()
                        .map(|expect| expectation(expect, &result, count));
                    let (exists, missing) = flag(reader.targets_exist(&question.justifies));
                    checks.justifies_exist = exists;
                    mismatch.or(missing)
                }
            },
        }
    };
    let status = checks.status(question.expect.is_some());
    Block {
        id: question.id.clone(),
        kind: "question".into(),
        status,
        checks,
        rows,
        elapsed_ms: u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX),
        reason: reason.or_else(|| {
            (status != Status::Pass).then(|| "one or more contract checks did not pass".into())
        }),
    }
}

/// Emit one report for every question. This never executes command definitions.
#[must_use]
pub fn questions(
    reader: &mut impl Reader,
    contract: &Contract,
    registry: &[SharedQuery],
) -> Report {
    Report::new(
        contract
            .questions
            .iter()
            .map(|q| question(reader, q, registry))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sparql::Bindings;

    struct Fixture {
        result: Option<Observation<QueryResult>>,
        exists: bool,
        calls: usize,
    }

    impl Reader for Fixture {
        fn query(&mut self, _: &str) -> Observation<QueryResult> {
            self.calls += 1;
            self.result.take().expect("one bounded query")
        }
        fn targets_exist(&mut self, _: &[String]) -> Observation<bool> {
            Observation::Known(self.exists)
        }
        fn graph_matches(&mut self, _: &str, _: &[Triple]) -> Observation<bool> {
            Observation::Unknown("shape evidence unavailable".into())
        }
        fn is_iri(&mut self, _: i64) -> Observation<bool> {
            Observation::Unknown("dictionary identity unavailable".into())
        }
    }

    fn fixture(result: Observation<QueryResult>) -> Fixture {
        Fixture {
            result: Some(result),
            exists: true,
            calls: 0,
        }
    }

    fn contract() -> Contract {
        super::super::parse(
            r#"
schema = "quipu-competency/1"
[[question]]
id = "count"
ask = "What is the count?"
query = "SELECT ?n WHERE { BIND(1 AS ?n) }"
returns = { n = "integer" }
justifies = ["urn:Count"]
expect = "rows=1"
"#,
            &[],
        )
        .unwrap()
    }

    fn select(value: Value) -> QueryResult {
        QueryResult::Select {
            variables: vec!["n".into()],
            rows: vec![Bindings::from_iter([("n".into(), value)])],
        }
    }

    #[test]
    fn complete_typed_result_passes_but_wrong_type_and_missing_binding_fail() {
        let c = contract();
        let mut reader = fixture(Observation::Known(select(Value::Int(1))));
        let report = questions(&mut reader, &c, &[]);
        assert!(report.successful());
        assert_eq!(reader.calls, 1);
        assert_eq!(report.blocks[0].rows, Some(1));
        for result in [
            select(Value::Str("1".into())),
            QueryResult::Select {
                variables: vec!["n".into()],
                rows: vec![Bindings::default()],
            },
        ] {
            let mut reader = fixture(Observation::Known(result));
            let report = questions(&mut reader, &c, &[]);
            assert_eq!(report.blocks[0].status, Status::Fail);
            assert_eq!(report.blocks[0].checks.returns_match, Some(false));
            assert!(!report.successful());
        }
    }

    #[test]
    fn timeout_and_408_observations_are_unknown_not_empty_or_failed() {
        for reason in ["query timeout", "HTTP 408", "partial result"] {
            let mut reader = fixture(Observation::Unknown(reason.into()));
            let report = questions(&mut reader, &contract(), &[]);
            assert_eq!(report.summary.unknown, 1);
            assert_eq!(report.summary.fail, 0);
            assert_eq!(report.blocks[0].checks.runs, None);
            assert_eq!(report.blocks[0].rows, None);
            assert!(!report.successful());
        }
    }

    #[test]
    fn negative_expectation_or_target_evidence_fails() {
        for target_missing in [false, true] {
            let mut c = contract();
            let mut reader = fixture(Observation::Known(select(Value::Int(1))));
            if target_missing {
                reader.exists = false;
            } else {
                c.questions[0].expect = Some("empty".into());
            }
            let report = questions(&mut reader, &c, &[]);
            assert_eq!(report.summary.fail, 1);
            assert!(!report.successful());
        }
    }

    #[test]
    fn malformed_or_retired_question_never_calls_reader() {
        for retired in [false, true] {
            let mut c = contract();
            if retired {
                c.questions[0].retired = true;
            } else {
                c.questions[0].query = Some("INSERT DATA {}".into());
            }
            let mut reader = fixture(Observation::Failed("must not execute".into()));
            let report = questions(&mut reader, &c, &[]);
            assert_eq!(reader.calls, 0);
            assert_eq!(
                report.blocks[0].status,
                if retired {
                    Status::Unknown
                } else {
                    Status::Fail
                }
            );
            assert!(!report.successful());
        }
    }
}
