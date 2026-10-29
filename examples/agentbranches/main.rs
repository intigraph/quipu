//! `agentbranches` — agent-run branches: N scripted agents each work on a fork
//! of one shared memory graph, then every fork is promoted.
//!
//! Measures, per arm: questions asked of a person (and how many the oracle
//! says were needed), silent losses, post-merge `sh:maxCount` violations, and
//! — for the arms that keep time — as-of answers about back-dated
//! corrections. No LLM anywhere: the agents are deterministic scripts, so the
//! run is its own ground truth. The `TerminusDB` arm consumes the scenario this
//! writes; see `benchmark/agent-branches/README.md`.
//!
//! ```bash
//! python3 benchmark/agent-branches/run.py          # everything, all seeds
//! cargo run --release --example agentbranches --features shacl -- --seed 42 --out /tmp/ab
//! ```

#![allow(dead_code)]

#[path = "../mergebench/generate.rs"]
mod generate;
#[path = "../mergebench/model.rs"]
mod model;
#[path = "../mergebench/rng.rs"]
mod rng;
mod scenario;
mod score;
#[path = "../mergebench/shapes.rs"]
mod shapes;
mod store_arm;
#[path = "../mergebench/strategies.rs"]
mod strategies;

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde_json::json;

use scenario::{Params, Scenario, plain, slot_key};
use score::{ArmMetrics, run_model_arm, score};
use store_arm::Mode;

/// The merge paper's strategies, run end to end on the model graphs.
const MODEL_ARMS: &[&str] = &[
    "git-canonical",
    "union",
    "lww-theirs",
    "triple-3way",
    "context-merge",
    "shape-aware",
    "two-way",
];

fn parse() -> (Params, PathBuf) {
    let mut p = Params::default();
    let mut out = PathBuf::from("benchmark/agent-branches/out");
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < args.len() {
        let v = args.get(i + 1).cloned().unwrap_or_default();
        match args[i].as_str() {
            "--seed" => p.seed = v.parse().expect("--seed N"),
            "--agents" => p.agents = v.parse().expect("--agents N"),
            "--entities" => p.entities = v.parse().expect("--entities N"),
            "--hot" => p.hot = v.parse().expect("--hot N"),
            "--edits" => p.edits = v.parse().expect("--edits N"),
            "--overlap" => p.overlap = v.parse().expect("--overlap F"),
            "--root-writes" => p.root_writes = v.parse().expect("--root-writes N"),
            "--out" => out = PathBuf::from(v),
            other => panic!("unknown argument {other}"),
        }
        i += 2;
    }
    (p, out)
}

fn ops_json(ops: &[scenario::Op]) -> Vec<serde_json::Value> {
    ops.iter()
        .map(|op| {
            json!({
                "writer": op.writer,
                "kind": op.kind,
                "entity": format!("e{}", op.entity),
                "predicate": op.predicate,
                "value": op.value.as_deref().map(plain),
                "valid_from": op.valid_from,
            })
        })
        .collect()
}

/// The scenario in a form the `TerminusDB` arm can replay: base documents, the
/// writes in order, and the corrections with their probe times.
fn scenario_json(sc: &Scenario) -> serde_json::Value {
    let slots = model::by_slot(&sc.base);
    let mut docs: BTreeMap<String, BTreeMap<String, Vec<String>>> = BTreeMap::new();
    for (slot, values) in &slots {
        let key = slot_key(slot);
        let (e, p) = key.split_once('|').unwrap_or_default();
        if p.ends_with("#type") {
            continue;
        }
        docs.entry(e.to_string())
            .or_default()
            .insert(p.to_string(), values.iter().map(|v| plain(v)).collect());
    }
    json!({
        "base": docs,
        "root": ops_json(&sc.root),
        "agents": sc.agents.iter().map(|ops| ops_json(ops)).collect::<Vec<_>>(),
        "corrections": sc.corrections.iter().map(|c| json!({
            "agent": c.agent,
            "entity": format!("e{}", c.entity),
            "predicate": c.predicate,
            "old": plain(&c.old),
            "new": plain(&c.new),
            "valid_from": c.valid_from,
            "before": c.before,
            "after": c.after,
        })).collect::<Vec<_>>(),
        "functional": shapes::FUNCTIONAL.iter().map(|p| p.name).collect::<Vec<_>>(),
        "bounded": shapes::BOUNDED.iter().map(|p| (p.name, p.max_count)).collect::<BTreeMap<_, _>>(),
    })
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (params, out) = parse();
    std::fs::create_dir_all(&out)?;
    let sc = scenario::generate(params);

    let mut arms: Vec<ArmMetrics> = Vec::new();
    let mut trajectories = BTreeMap::new();
    for arm in MODEL_ARMS {
        let t = run_model_arm(&sc, arm);
        arms.push(score(&sc, arm, "model", &t));
        trajectories.insert((*arm).to_string(), t);
    }

    let three = store_arm::run(&sc, Mode::ThreeWay, "quipu-promote-3way")?;
    let two = store_arm::run(&sc, Mode::TwoWay, "quipu-promote-2way-pre888")?;
    let direct = store_arm::run(&sc, Mode::Direct, "quipu-direct-root")?;
    arms.push(score(&sc, "quipu-promote-3way", "store", &three.trajectory));
    arms.push(score(
        &sc,
        "quipu-promote-2way-pre888",
        "store",
        &two.trajectory,
    ));

    // Instrument controls: the real store must agree with the model of the
    // operator it ships (shape-aware) and of the one it replaced (two-way).
    let same = |a: &score::Trajectory, b: &score::Trajectory| {
        a.states == b.states
            && a.promotions.len() == b.promotions.len()
            && a.promotions
                .iter()
                .zip(&b.promotions)
                .all(|(x, y)| x.asked == y.asked && x.needed == y.needed)
    };
    let controls = json!({
        "store_3way_equals_model_shape_aware": same(&three.trajectory, &trajectories["shape-aware"]),
        "store_2way_equals_model_two_way": same(&two.trajectory, &trajectories["two-way"]),
        "shacl_refusals_at_promote": three.refused,
    });

    let mut probes = three.probes;
    probes.extend(two.probes);
    probes.extend(direct.probes);

    let doc = json!({
        "params": params,
        "controls": controls,
        "arms": arms,
        "probes": probes,
        "scenario": scenario_json(&sc),
    });
    let path = out.join(format!("agentbranches-seed{}.json", params.seed));
    std::fs::write(&path, serde_json::to_string_pretty(&doc)?)?;

    println!(
        "seed {}: {} agents, {} writes -> {}",
        params.seed,
        params.agents,
        arms[0].writes,
        path.display()
    );
    println!(
        "{:<28} {:>9} {:>6} {:>6} {:>6} {:>6} {:>10}",
        "arm", "questions", "needed", "missed", "extra", "lost", "violations"
    );
    for a in &arms {
        println!(
            "{:<28} {:>9} {:>6} {:>6} {:>6} {:>6} {:>10}",
            a.arm, a.questions, a.needed, a.missed, a.unnecessary, a.silent_losses, a.violations
        );
    }
    let mut by_arm: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
    for p in &probes {
        let e = by_arm.entry(&p.arm).or_default();
        e.1 += 1;
        if p.ok {
            e.0 += 1;
        }
    }
    for (arm, (ok, n)) in by_arm {
        println!("as-of {arm:<28} {ok}/{n} correct");
    }
    println!("controls {controls}");
    if controls["store_3way_equals_model_shape_aware"] != json!(true)
        || controls["store_2way_equals_model_two_way"] != json!(true)
    {
        return Err("instrument control failed: store and model disagree".into());
    }
    Ok(())
}
