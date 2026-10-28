//! The store arms: the same scenario run through a real `quipu::Store` —
//! `fork_create`, writes into each fork, `fork_promote` — and the as-of probes
//! read back through the public SPARQL path (`query_temporal`).
//!
//! `ThreeWay` is the shipped promote (#888). `TwoWay` is the negative control:
//! the pre-#888 promote, reimplemented here from its definition (apply
//! `fork_diff(main, fork)` to ROOT) rather than built from old code. `Direct`
//! skips forks entirely and writes each correction straight into ROOT; it only
//! answers the as-of probes, and isolates what promotion does to them.

use std::collections::{BTreeSet, HashMap};

use quipu::sparql::{QueryResult, TemporalContext, query_temporal};
use quipu::store::forks::ForkPromotion;
use quipu::{Datum, Op, Store, Value};
use serde::Serialize;

use crate::model::{Graph, Slot, Term, Triple, iri, lit};
use crate::scenario::{
    BASE_TS, Correction, FORK_TS, Kind, Op as ScriptOp, ROOT_TS, Scenario, agent_ts, entity, plain,
    pred, promote_ts, resolve_ts,
};
use crate::score::{Promotion, Trajectory, needed};
use crate::shapes::{self, NS};

type Res<T> = Result<T, Box<dyn std::error::Error>>;

/// Which promote a store arm runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// The shipped three-way promote.
    ThreeWay,
    /// The pre-#888 two-way promote (negative control).
    TwoWay,
    /// No forks: corrections written straight into ROOT (as-of control).
    Direct,
}

/// One as-of question and its answer.
#[derive(Debug, Clone, Serialize)]
pub struct Probe {
    /// Arm name.
    pub arm: String,
    /// The agent whose correction is asked about.
    pub agent: usize,
    /// `valid-before`, `valid-after`, `current` or `belief-before-landing`.
    pub probe: &'static str,
    /// What a correct bitemporal store answers.
    pub expected: Vec<String>,
    /// What this arm answered.
    pub got: Vec<String>,
    /// `got == expected`.
    pub ok: bool,
}

/// A store arm's trajectory, as-of answers and refusals.
pub struct StoreRun {
    /// Questions, oracle and final ROOT.
    pub trajectory: Trajectory,
    /// The as-of answers.
    pub probes: Vec<Probe>,
    /// SHACL refusals at promote (each one also needs a person).
    pub refused: usize,
}

fn term_value(store: &Store, term: &Term) -> Res<Value> {
    if let Some(inner) = term.strip_prefix('<').and_then(|t| t.strip_suffix('>')) {
        return Ok(Value::Ref(store.intern(inner)?));
    }
    Ok(Value::Str(plain(term)))
}

fn value_term(store: &Store, v: &Value, cache: &mut HashMap<i64, String>) -> Res<Option<Term>> {
    Ok(match v {
        Value::Ref(id) => Some(iri(&resolve(store, *id, cache)?)),
        Value::Str(s) => Some(lit(s)),
        _ => None,
    })
}

fn resolve(store: &Store, id: i64, cache: &mut HashMap<i64, String>) -> Res<String> {
    if let Some(s) = cache.get(&id) {
        return Ok(s.clone());
    }
    let s = store.resolve(id)?;
    cache.insert(id, s.clone());
    Ok(s)
}

/// A graph's present state, restricted to the bench vocabulary.
fn export(store: &Store, g: i64) -> Res<Graph> {
    let mut cache = HashMap::new();
    let mut out = Graph::new();
    for f in store.current_facts_in_graph(g)? {
        let s = resolve(store, f.entity, &mut cache)?;
        if !s.starts_with(NS) {
            continue;
        }
        let p = resolve(store, f.attribute, &mut cache)?;
        if let Some(o) = value_term(store, &f.value, &mut cache)? {
            out.insert(Triple::new(s, p, o));
        }
    }
    Ok(out)
}

fn datum(e: i64, a: i64, value: Value, valid_from: &str, op: Op) -> Datum {
    Datum {
        entity: e,
        attribute: a,
        value,
        valid_from: valid_from.to_string(),
        valid_to: None,
        op,
    }
}

/// Set slot `(e, a)` in graph `g` to exactly `values`, in one transaction at
/// `ts`. Retractions close the old values at `ts`.
fn set_slot(
    store: &mut Store,
    g: i64,
    slot: &Slot,
    values: &BTreeSet<Term>,
    ts: &str,
    actor: &str,
) -> Res<()> {
    let e = store.intern(&slot.0)?;
    let a = store.intern(&slot.1)?;
    let mut want = Vec::with_capacity(values.len());
    for v in values {
        want.push(term_value(store, v)?);
    }
    let mut datums = Vec::new();
    let mut have = Vec::new();
    for f in store.current_facts_in_graph(g)? {
        if f.entity == e && f.attribute == a {
            if !want.contains(&f.value) {
                datums.push(datum(e, a, f.value.clone(), &f.valid_from, Op::Retract));
            }
            have.push(f.value);
        }
    }
    for v in want {
        if !have.contains(&v) {
            datums.push(datum(e, a, v, ts, Op::Assert));
        }
    }
    if !datums.is_empty() {
        store.transact_to_graph(&datums, ts, Some(actor), Some(actor), g)?;
    }
    Ok(())
}

/// Apply one scripted write to graph `g`.
fn write_op(store: &mut Store, g: i64, op: &ScriptOp, ts: &str) -> Res<()> {
    let slot = op.slot();
    if op.kind == Kind::Add {
        let e = store.intern(&slot.0)?;
        let a = store.intern(&slot.1)?;
        let v = term_value(store, op.value.as_ref().ok_or("add without a value")?)?;
        store.transact_to_graph(
            &[datum(e, a, v, ts, Op::Assert)],
            ts,
            Some(&op.writer),
            Some(&op.writer),
            g,
        )?;
        return Ok(());
    }
    let values: BTreeSet<Term> = op.value.iter().cloned().collect();
    set_slot(store, g, &slot, &values, ts, &op.writer)
}

fn values_in(g: &Graph, slot: &Slot) -> BTreeSet<Term> {
    g.iter()
        .filter(|t| t.s == slot.0 && t.p == slot.1)
        .map(|t| t.o.clone())
        .collect()
}

/// Read one slot through SPARQL at a valid time and/or transaction cut.
fn ask(
    store: &Store,
    c: &Correction,
    valid_at: Option<&str>,
    as_of_tx: Option<i64>,
) -> Res<Vec<String>> {
    let q = format!(
        "SELECT ?o WHERE {{ <{}> <{}> ?o }}",
        entity(c.entity),
        pred(c.predicate)
    );
    let ctx = TemporalContext {
        valid_at: valid_at.map(str::to_string),
        as_of_tx,
        ..Default::default()
    };
    let mut cache = HashMap::new();
    let mut out = BTreeSet::new();
    if let QueryResult::Select { rows, .. } = query_temporal(store, &q, &ctx)? {
        for row in rows {
            if let Some(v) = row.get("o")
                && let Some(t) = value_term(store, v, &mut cache)?
            {
                out.insert(plain(&t));
            }
        }
    }
    Ok(out.into_iter().collect())
}

/// One as-of question: name, valid time, transaction cut, expected answer.
type ProbeCase<'a> = (&'static str, Option<&'a str>, Option<i64>, &'a Vec<String>);

/// The four as-of questions for each correction. `landed[a]` is the last
/// transaction before agent `a`'s correction reached ROOT.
fn probes(store: &Store, sc: &Scenario, arm: &str, landed: &[i64]) -> Res<Vec<Probe>> {
    let mut out = Vec::new();
    for c in &sc.corrections {
        let old = vec![plain(&c.old)];
        let new = vec![plain(&c.new)];
        let cases: [ProbeCase<'_>; 4] = [
            ("valid-before", Some(&c.before), None, &old),
            ("valid-after", Some(&c.after), None, &new),
            ("current", None, None, &new),
            (
                "belief-before-landing",
                Some(&c.after),
                Some(landed[c.agent]),
                &old,
            ),
        ];
        for (name, valid_at, as_of_tx, expected) in cases {
            let got = ask(store, c, valid_at, as_of_tx)?;
            out.push(Probe {
                arm: arm.to_string(),
                agent: c.agent,
                probe: name,
                ok: &got == expected,
                expected: expected.clone(),
                got,
            });
        }
    }
    Ok(out)
}

fn seeded_store(sc: &Scenario) -> Res<(Store, i64)> {
    let mut store = Store::open_in_memory()?;
    store.load_shapes("agentbranches", &shapes::turtle(), BASE_TS)?;
    let mut datums = Vec::with_capacity(sc.base.len());
    for t in &sc.base {
        let e = store.intern(&t.s)?;
        let a = store.intern(&t.p)?;
        let v = term_value(&store, &t.o)?;
        datums.push(datum(e, a, v, BASE_TS, Op::Assert));
    }
    let tx = store.transact(&datums, BASE_TS, Some("base"), Some("base"))?;
    Ok((store, tx))
}

/// Run one store arm.
///
/// # Errors
/// Any store error; the harness never swallows one.
pub fn run(sc: &Scenario, mode: Mode, arm: &str) -> Res<StoreRun> {
    let (mut store, base_tx) = seeded_store(sc)?;

    if mode == Mode::Direct {
        let mut landed = vec![0; sc.corrections.len()];
        for c in &sc.corrections {
            landed[c.agent] = store.latest_tx_id()?;
            let slot = (entity(c.entity), pred(c.predicate));
            let values = BTreeSet::from([c.new.clone()]);
            set_slot(&mut store, 0, &slot, &values, &c.valid_from, "direct")?;
        }
        let probes = probes(&store, sc, arm, &landed)?;
        return Ok(StoreRun {
            trajectory: Trajectory::default(),
            probes,
            refused: 0,
        });
    }

    let mut forks = Vec::with_capacity(sc.agents.len());
    for a in 0..sc.agents.len() {
        let name = format!("agent-{a}");
        let f = store.fork_create(&name, base_tx, FORK_TS, Some(&name))?;
        forks.push((name, f.g));
    }
    for op in &sc.root {
        write_op(&mut store, 0, op, ROOT_TS)?;
    }
    for (a, ops) in sc.agents.iter().enumerate() {
        for op in ops {
            // A back-dated correction is written AT its valid time, the way
            // the temporal model documents a correction.
            let ts = op.valid_from.clone().unwrap_or_else(|| agent_ts(a));
            write_op(&mut store, forks[a].1, op, &ts)?;
        }
    }

    let mut promotions = Vec::with_capacity(forks.len());
    let mut states = vec![export(&store, 0)?];
    let mut landed = vec![0; forks.len()];
    let mut refused = 0;
    for (a, (name, g)) in forks.iter().enumerate() {
        let ours = export(&store, 0)?;
        let theirs = export(&store, *g)?;
        let need = needed(&sc.base, &ours, &theirs);
        landed[a] = store.latest_tx_id()?;
        let mut p = Promotion {
            needed: need,
            ..Promotion::default()
        };
        match mode {
            Mode::ThreeWay => loop {
                p.rounds += 1;
                match store.fork_promote(name, &promote_ts(a), Some(name))? {
                    ForkPromotion::Promoted { .. } => break,
                    ForkPromotion::Conflicts(records) => {
                        // The person rules for the promoting agent on each
                        // conflicted slot, then promotion is retried.
                        let fork_now = export(&store, *g)?;
                        for r in records {
                            let s = r.subject.trim_start_matches('<').trim_end_matches('>');
                            let slot = (s.to_string(), r.predicate.clone());
                            let values = values_in(&fork_now, &slot);
                            set_slot(
                                &mut store,
                                0,
                                &slot,
                                &values,
                                &resolve_ts(a, p.rounds),
                                "person",
                            )?;
                            p.asked.insert(slot);
                        }
                    }
                    ForkPromotion::Refused(_) => {
                        refused += 1;
                        break;
                    }
                }
                if p.rounds > 4 {
                    return Err(format!("{name}: promotion did not settle in 4 rounds").into());
                }
            },
            Mode::TwoWay => {
                p.rounds = 1;
                let diff = store.fork_diff("main", name)?;
                let mut datums = Vec::with_capacity(diff.added.len() + diff.removed.len());
                for f in &diff.added {
                    datums.push(datum(
                        f.entity,
                        f.attribute,
                        f.value.clone(),
                        &f.valid_from,
                        Op::Assert,
                    ));
                }
                for f in &diff.removed {
                    datums.push(datum(
                        f.entity,
                        f.attribute,
                        f.value.clone(),
                        &f.valid_from,
                        Op::Retract,
                    ));
                }
                if !datums.is_empty() {
                    let source = format!("fork-promote-2way:{name}");
                    store.transact(&datums, &promote_ts(a), Some(name), Some(&source))?;
                }
            }
            Mode::Direct => unreachable!(),
        }
        promotions.push(p);
        states.push(export(&store, 0)?);
    }
    let probes = probes(&store, sc, arm, &landed)?;
    Ok(StoreRun {
        trajectory: Trajectory { promotions, states },
        probes,
        refused,
    })
}
