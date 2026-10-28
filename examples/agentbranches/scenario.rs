//! The agent-run-branches scenario: one shared memory graph, N scripted
//! agents each editing their own fork of it, a live writer that keeps writing
//! to ROOT while the agents work, and one back-dated correction per agent.
//!
//! Every edit is drawn from one `SplitMix64` stream seeded by `--seed`; there
//! is no other entropy and no model in the loop. The vocabulary and the shapes
//! are the merge paper's (`examples/mergebench/shapes.rs`), so "a slot", "a
//! conflict" and "the oracle" mean exactly what they mean there.

use std::collections::BTreeMap;

use serde::Serialize;

use crate::model::{Graph, Slot, Term, Triple, by_slot, iri, lit, rdf_type};
use crate::rng::SplitMix64;
use crate::shapes::NS;

/// Valid time of every base fact.
pub const BASE_TS: &str = "2026-01-01T00:00:00Z";
/// When the forks are taken.
pub const FORK_TS: &str = "2026-10-01T00:00:00Z";
/// When the live writer writes to ROOT (after the forks exist).
pub const ROOT_TS: &str = "2026-10-02T00:00:00Z";

/// When agent `a` does its ordinary (not back-dated) work.
#[must_use]
pub fn agent_ts(a: usize) -> String {
    format!("2026-10-03T00:{a:02}:00Z")
}

/// When agent `a`'s fork is promoted.
#[must_use]
pub fn promote_ts(a: usize) -> String {
    format!("2026-10-27T12:{a:02}:00Z")
}

/// When the person settles agent `a`'s questions.
#[must_use]
pub fn resolve_ts(a: usize, round: usize) -> String {
    format!("2026-10-27T12:{a:02}:{:02}Z", 10 + round)
}

/// Generator parameters, echoed into every results file.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Params {
    /// The only entropy in the run.
    pub seed: u64,
    /// Number of agents, one fork each.
    pub agents: usize,
    /// Entities in the shared memory graph.
    pub entities: usize,
    /// Entities every agent may touch (the contended set).
    pub hot: usize,
    /// Ordinary edits per agent (the correction is extra).
    pub edits: usize,
    /// Fraction of an agent's edits aimed at the contended set.
    pub overlap: f64,
    /// Writes the live writer makes to ROOT while the agents work.
    pub root_writes: usize,
}

impl Default for Params {
    fn default() -> Self {
        Self {
            seed: 42,
            agents: 8,
            entities: 48,
            hot: 4,
            edits: 10,
            overlap: 0.5,
            root_writes: 16,
        }
    }
}

/// What an edit does to its slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// Replace a single-valued slot's value.
    Set,
    /// Remove a single-valued slot's value.
    Clear,
    /// Add one value to a multi-valued slot.
    Add,
    /// Replace a single-valued slot's value, effective at a PAST valid time.
    Correct,
}

/// One scripted write.
#[derive(Debug, Clone, Serialize)]
pub struct Op {
    /// `agent-<n>` or `root` (the live writer).
    pub writer: String,
    /// What the write does.
    pub kind: Kind,
    /// Entity index (`bench:e<n>`).
    pub entity: usize,
    /// Predicate local name under the bench namespace.
    pub predicate: &'static str,
    /// The value as an N-Triples term (`<iri>` or `"literal"`); `None` for Clear.
    pub value: Option<Term>,
    /// For a correction, the valid time it takes effect from.
    pub valid_from: Option<String>,
}

impl Op {
    /// The slot the write lands in.
    #[must_use]
    pub fn slot(&self) -> Slot {
        (entity(self.entity), pred(self.predicate))
    }
}

/// One back-dated correction and the four questions asked about it.
#[derive(Debug, Clone, Serialize)]
pub struct Correction {
    /// The agent that made it.
    pub agent: usize,
    /// Entity index.
    pub entity: usize,
    /// Predicate local name.
    pub predicate: &'static str,
    /// The value before the correction (base).
    pub old: Term,
    /// The corrected value.
    pub new: Term,
    /// The valid time the correction takes effect from.
    pub valid_from: String,
    /// A valid time before `valid_from` (expect `old`).
    pub before: String,
    /// A valid time after `valid_from` and before any promotion (expect `new`).
    pub after: String,
}

/// The whole run's input.
pub struct Scenario {
    /// Parameters.
    pub params: Params,
    /// The shared memory graph every fork starts from.
    pub base: Graph,
    /// Each agent's writes, in order.
    pub agents: Vec<Vec<Op>>,
    /// The live writer's writes to ROOT, made after the forks are taken.
    pub root: Vec<Op>,
    /// One back-dated correction per agent (also present in `agents`).
    pub corrections: Vec<Correction>,
}

/// Entity IRI.
#[must_use]
pub fn entity(i: usize) -> String {
    format!("{NS}e{i}")
}

/// Predicate IRI.
#[must_use]
pub fn pred(name: &str) -> String {
    format!("{NS}{name}")
}

const FUNCTIONAL: [&str; 4] = ["label", "status", "owner", "version"];

/// The shared memory graph: every entity typed, every declared predicate
/// populated, two of the four allowed tags in place. No randomness.
#[must_use]
pub fn base_graph(n: usize) -> Graph {
    let mut g = Graph::new();
    for i in 0..n {
        let s = entity(i);
        g.insert(Triple::new(&s, rdf_type(), iri(&format!("{NS}Entity"))));
        g.insert(Triple::new(&s, pred("label"), lit(&format!("entity {i}"))));
        g.insert(Triple::new(&s, pred("status"), lit("active")));
        g.insert(Triple::new(
            &s,
            pred("owner"),
            iri(&format!("{NS}team{}", i % 5)),
        ));
        g.insert(Triple::new(&s, pred("version"), lit("1")));
        g.insert(Triple::new(&s, pred("tag"), lit(&format!("t{}", i % 7))));
        g.insert(Triple::new(
            &s,
            pred("tag"),
            lit(&format!("t{}", (i + 3) % 7)),
        ));
        g.insert(Triple::new(&s, pred("note"), lit(&format!("note {i}"))));
        g.insert(Triple::new(
            &s,
            pred("relatedTo"),
            iri(&entity((i + 1) % n)),
        ));
    }
    g
}

/// Apply one write to a model graph.
pub fn apply(g: &mut Graph, op: &Op) {
    let (s, p) = op.slot();
    match op.kind {
        Kind::Set | Kind::Correct | Kind::Clear => {
            g.retain(|t| !(t.s == s && t.p == p));
            if let Some(v) = &op.value {
                g.insert(Triple::new(&s, &p, v));
            }
        }
        Kind::Add => {
            if let Some(v) = &op.value {
                g.insert(Triple::new(&s, &p, v));
            }
        }
    }
}

fn below(rng: &mut SplitMix64, n: usize) -> usize {
    usize::try_from(rng.below(n as u64)).unwrap_or(0)
}

fn day(month: usize, d: usize) -> String {
    format!("2026-{month:02}-{d:02}T00:00:00Z")
}

/// Generate the scenario.
///
/// # Panics
/// When the parameters leave an agent without an entity of its own.
#[must_use]
pub fn generate(p: Params) -> Scenario {
    assert!(
        p.entities >= p.hot + p.agents,
        "need at least one uncontended entity per agent"
    );
    let mut rng = SplitMix64::new(p.seed);
    let base = base_graph(p.entities);
    let base_slots = by_slot(&base);
    let mut agents = Vec::with_capacity(p.agents);
    let mut corrections = Vec::with_capacity(p.agents);

    for a in 0..p.agents {
        let writer = format!("agent-{a}");
        // An agent's own entities: never touched by another agent, so every
        // edit there is a non-conflicting concurrent edit.
        let own: Vec<usize> = (p.hot..p.entities)
            .filter(|j| (j - p.hot) % p.agents == a)
            .collect();

        // The late correction: "status was actually X since <past date>".
        let corr_e = own[below(&mut rng, own.len())];
        let month = 3 + below(&mut rng, 6);
        let d = 1 + below(&mut rng, 28);
        let correction = Correction {
            agent: a,
            entity: corr_e,
            predicate: "status",
            old: base_slots[&(entity(corr_e), pred("status"))]
                .iter()
                .next()
                .cloned()
                .unwrap_or_default(),
            new: lit(&format!("corrected-a{a}")),
            valid_from: day(month, d),
            before: day(month - 1, d),
            after: day(month + 1, d),
        };
        let corr_pos = below(&mut rng, p.edits + 1);
        let corr_op = Op {
            writer: writer.clone(),
            kind: Kind::Correct,
            entity: corr_e,
            predicate: "status",
            value: Some(correction.new.clone()),
            valid_from: Some(correction.valid_from.clone()),
        };

        let mut ops = Vec::with_capacity(p.edits + 1);
        let mut tag_adds: BTreeMap<usize, usize> = BTreeMap::new();
        for k in 0..p.edits {
            if k == corr_pos {
                ops.push(corr_op.clone());
            }
            let contended = (rng.below(1000) as f64) / 1000.0 < p.overlap;
            let e = if contended {
                below(&mut rng, p.hot)
            } else {
                own[below(&mut rng, own.len())]
            };
            let functional = |rng: &mut SplitMix64| {
                let f = FUNCTIONAL[below(rng, FUNCTIONAL.len())];
                // The correction's slot is that agent's alone, so the as-of
                // probes measure time semantics and nothing else.
                if e == corr_e && f == "status" {
                    "version"
                } else {
                    f
                }
            };
            let roll = below(&mut rng, 10);
            let op = match roll {
                0..=3 => {
                    let f = functional(&mut rng);
                    let value = if f == "owner" {
                        iri(&format!("{NS}team-a{a}k{k}"))
                    } else {
                        lit(&format!("a{a}k{k}"))
                    };
                    (Kind::Set, f, Some(value))
                }
                4 => (Kind::Clear, functional(&mut rng), None),
                // At most two tag additions per agent per entity, so every
                // fork is valid on its own (2 base + 2 <= sh:maxCount 4); only
                // the union across agents can overflow.
                5 | 6 if tag_adds.get(&e).copied().unwrap_or(0) < 2 => {
                    *tag_adds.entry(e).or_default() += 1;
                    (Kind::Add, "tag", Some(lit(&format!("a{a}tag{k}"))))
                }
                9 => (
                    Kind::Add,
                    "relatedTo",
                    Some(iri(&entity(below(&mut rng, p.entities)))),
                ),
                _ => (Kind::Add, "note", Some(lit(&format!("a{a} note {k}")))),
            };
            ops.push(Op {
                writer: writer.clone(),
                kind: op.0,
                entity: e,
                predicate: op.1,
                value: op.2,
                valid_from: None,
            });
        }
        if corr_pos == p.edits {
            ops.push(corr_op);
        }
        agents.push(ops);
        corrections.push(correction);
    }

    // The live writer: keeps writing to ROOT after the forks are taken. Its
    // notes are what the pre-#888 two-way promote silently retracted; its
    // version bumps collide with agents that also bumped a version.
    let mut root = Vec::with_capacity(p.root_writes);
    for k in 0..p.root_writes {
        let e = below(&mut rng, p.entities);
        let op = if below(&mut rng, 3) == 2 {
            (Kind::Set, "version", lit(&format!("r{k}")))
        } else {
            (Kind::Add, "note", lit(&format!("root note {k}")))
        };
        root.push(Op {
            writer: "root".to_string(),
            kind: op.0,
            entity: e,
            predicate: op.1,
            value: Some(op.2),
            valid_from: None,
        });
    }

    Scenario {
        params: p,
        base,
        agents,
        root,
        corrections,
    }
}

/// A plain rendering of a term for the JSON hand-off: IRIs in the bench
/// namespace lose the namespace, literals lose their quotes.
#[must_use]
pub fn plain(term: &str) -> String {
    if let Some(inner) = term.strip_prefix('<').and_then(|t| t.strip_suffix('>')) {
        return inner.strip_prefix(NS).unwrap_or(inner).to_string();
    }
    if let Some(inner) = term.strip_prefix('"').and_then(|t| t.strip_suffix('"')) {
        return inner.replace("\\\"", "\"").replace("\\\\", "\\");
    }
    term.to_string()
}

/// `e<n>|<predicate>`: the slot key the JSON hand-off and the Python scorer use.
#[must_use]
pub fn slot_key(slot: &Slot) -> String {
    format!(
        "{}|{}",
        plain(&format!("<{}>", slot.0)),
        plain(&format!("<{}>", slot.1))
    )
}
