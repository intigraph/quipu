//! Scratch-only fork adapter. Input recipes contain no expected answers.
use quipu::store::forks::{ForkPromotion, ForkResolutions};
use quipu::{Datum, Op, Store, Value};
use serde::Deserialize;
use serde_json::json;
use std::collections::BTreeMap;
use std::io::{self, Read};

mod observe;
type Res<T> = Result<T, Box<dyn std::error::Error>>;
const SUBJECT: &str = "urn:oracle:item";
const PREDICATE: &str = "urn:oracle:value";
const FORK: &str = "oracle-input";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Write {
    value: String,
    from: String,
    to: Option<String>,
    op: WriteOp,
}
#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum WriteOp {
    Assert,
    Retract,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Step {
    at: String,
    rows: Vec<Write>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Probe {
    tx: String,
    valid_at: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    schema: String,
    case: String,
    path: String,
    max_count: Option<usize>,
    base: Step,
    fork_at: String,
    ours: Vec<Step>,
    theirs: Vec<Step>,
    decision_at: String,
    /// Explicit input values, not an expected result.
    resolution: Option<Vec<String>>,
    probes: Vec<Probe>,
}

fn write(store: &mut Store, step: &Step, graph: i64, e: i64, a: i64) -> Res<()> {
    if step.rows.is_empty() || step.rows.len() > 64 {
        return Err("write row bound".into());
    }
    let rows: Vec<Datum> = step
        .rows
        .iter()
        .map(|r| Datum {
            entity: e,
            attribute: a,
            value: Value::Str(r.value.clone()),
            valid_from: r.from.clone(),
            valid_to: r.to.clone(),
            op: match r.op {
                WriteOp::Assert => Op::Assert,
                WriteOp::Retract => Op::Retract,
            },
        })
        .collect();
    store.transact_to_graph(
        &rows,
        &step.at,
        Some("oracle-input"),
        Some("oracle-setup"),
        graph,
    )?;
    Ok(())
}

fn run(request: Request) -> Res<serde_json::Value> {
    if request.schema != "quipu-native-input/v1"
        || request.path != "fork-promote"
        || request.case.is_empty()
        || request.case.len() > 128
        || request.ours.len() > 16
        || request.theirs.len() > 16
        || request.probes.len() > 16
        || request.resolution.as_ref().is_some_and(|v| v.len() > 64)
    {
        return Err("unsupported or oversized native recipe".into());
    }
    // The caller cannot supply a database path. This store is always private scratch.
    let scratch = tempfile::tempdir()?;
    let db = scratch.path().join("fixture.db");
    let mut store = Store::open(db.to_str().ok_or("scratch path encoding")?)?;
    let bound = request
        .max_count
        .map(|n| format!("; sh:maxCount {n}"))
        .unwrap_or_default();
    let shapes = format!(
        "@prefix sh: <http://www.w3.org/ns/shacl#> . <urn:oracle:S> a sh:NodeShape ; sh:targetSubjectsOf <{PREDICATE}> ; sh:property [ sh:path <{PREDICATE}> {bound} ] ."
    );
    store.load_shapes("oracle-input", &shapes, &request.base.at)?;
    let e = store.intern(SUBJECT)?;
    let a = store.intern(PREDICATE)?;
    write(&mut store, &request.base, 0, e, a)?;
    let base_tx = store.latest_tx_id()?;
    let fork = store.fork_create(FORK, base_tx, &request.fork_at, Some("oracle-input"))?;
    for step in &request.ours {
        write(&mut store, step, 0, e, a)?;
    }
    for step in &request.theirs {
        write(&mut store, step, fork.g, e, a)?;
    }
    let before = store.latest_tx_id()?;
    let (_, conflicts) = store.fork_conflicts(FORK)?;
    let result = if let Some(values) = &request.resolution {
        if conflicts.len() != 1 {
            return Err("explicit resolution needs exactly one native conflict".into());
        }
        let chosen =
            ForkResolutions::from([((e, a), values.iter().cloned().map(Value::Str).collect())]);
        let digest = ring::digest::digest(&ring::digest::SHA256, &serde_json::to_vec(values)?);
        let hash = format!("sha256:{}", hex::encode(digest.as_ref()));
        store.fork_promote_resolved(
            FORK,
            &chosen,
            &hash,
            "oracle-input",
            &request.decision_at,
            Some("oracle-input"),
            false,
        )
    } else {
        store.fork_promote(FORK, &request.decision_at, Some("oracle-input"))
    };
    let native = match result {
        Ok(ForkPromotion::Promoted {
            tx,
            asserted,
            retracted,
        }) => json!({"outcome":"promoted","tx":tx,"asserted":asserted,"retracted":retracted}),
        Ok(ForkPromotion::Conflicts(records)) => json!({"outcome":"conflicts","records":records}),
        Ok(ForkPromotion::Refused(feedback)) => {
            json!({"outcome":"refused","feedback":format!("{feedback:?}")})
        }
        Err(error) => json!({"outcome":"error","error":error.to_string()}),
    };
    let after = store.latest_tx_id()?;
    let mut observation =
        observe::capture(&store, &db, e, a, before, after, base_tx, &request.probes)?;
    // Clean-slot classification is not exposed by this path; do not invent it.
    let mut report = serde_json::Map::new();
    if conflicts.len() == 1 {
        use quipu::share_merge_decisions_view::{ConflictKind, kind};
        let record = &conflicts[0];
        report.insert(
            "kind".into(),
            json!(match kind(record) {
                ConflictKind::MaxCountExceeded => "maxcount-exceeded",
                ConflictKind::DeleteReplace => "delete-replace",
            }),
        );
        if native["outcome"] == "promoted" {
            report.insert("held".into(), json!(0));
            if let Some(values) = &request.resolution {
                let chosen: std::collections::BTreeSet<_> = values
                    .iter()
                    .map(|v| oxrdf::Literal::new_simple_literal(v.clone()).to_string())
                    .collect();
                let side = |values: &[String]| {
                    values
                        .iter()
                        .cloned()
                        .collect::<std::collections::BTreeSet<_>>()
                };
                let decision = if chosen == side(&record.ours) {
                    "ours"
                } else if chosen == side(&record.theirs) {
                    "theirs"
                } else if chosen == side(&record.base) {
                    "base"
                } else {
                    "value"
                };
                report.insert("decision".into(), json!(decision));
            }
        } else if native["outcome"] == "conflicts" {
            report.insert("held".into(), json!(conflicts.len()));
        }
    }
    observation["report"] = serde_json::Value::Object(report);
    let capture = json!({"schema":"quipu-native-capture/v1","case":request.case,
        "path":"fork-promote","native":native,"conflicts":conflicts,
        "before_tx":before,"after_tx":after,"capture":observation});
    drop(store);
    scratch.close()?;
    Ok(capture)
}

fn main() -> Res<()> {
    let mut bytes = Vec::new();
    io::stdin().take(65_537).read_to_end(&mut bytes)?;
    if bytes.len() > 65_536 {
        return Err("input exceeds 64KiB".into());
    }
    let request: Request = serde_json::from_slice(&bytes)?;
    println!("{}", serde_json::to_string(&run(request)?)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn recipe() -> serde_json::Value {
        serde_json::from_str(include_str!(
            "../../benchmark/usecase-oracle/native-c4-input.json"
        ))
        .unwrap()
    }

    #[test]
    fn expected_answers_and_external_database_paths_are_not_input_fields() {
        for key in ["expected_current", "database"] {
            let mut input = recipe();
            input[key] = json!([]);
            assert!(serde_json::from_value::<Request>(input).is_err());
        }
    }

    #[test]
    fn c4_native_receipts_and_pinned_queries_are_observed_separately() {
        let result = run(serde_json::from_value(recipe()).unwrap()).unwrap();
        assert_eq!(result["native"]["outcome"], "promoted");
        let capture = &result["capture"];
        assert_eq!(capture["current"], json!(["c"]));
        assert_eq!(capture["native_transactions"].as_array().unwrap().len(), 2);
        assert_eq!(capture["transactions"][0]["role"], "receive");
        assert_eq!(capture["transactions"][1]["role"], "decide");
        assert_eq!(capture["as_of"][0]["values"], json!(["b"]));
        assert_eq!(capture["as_of"][1]["values"], json!(["c"]));
        assert_eq!(capture["as_of"][2]["values"], json!(["a"]));
        assert_eq!(capture["history"].as_array().unwrap().len(), 4);
    }

    #[test]
    fn unavailable_receipt_pin_refuses_instead_of_using_latest() {
        let mut input = recipe();
        input["probes"][0]["tx"] = json!("not-a-receipt");
        assert!(run(serde_json::from_value(input).unwrap()).is_err());
    }

    #[test]
    fn undecided_native_conflict_records_no_committed_transactions() {
        let mut input = recipe();
        input["resolution"] = json!(null);
        input["probes"] = json!([]);
        let result = run(serde_json::from_value(input).unwrap()).unwrap();
        assert_eq!(result["native"]["outcome"], "conflicts");
        assert_eq!(result["before_tx"], result["after_tx"]);
        assert_eq!(result["capture"]["transactions"], json!([]));
        assert_eq!(result["capture"]["current"], json!(["b"]));
    }
}
