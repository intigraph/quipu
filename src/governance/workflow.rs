//! Opt-in workflow processing at the transaction boundary.
//!
//! Capture current run state inside the savepoint, before staging; validate
//! the signed event and resulting state before commit. All ordinary write
//! surfaces use this guard, including SPARQL Update. Definitions, events and
//! runs share the existing Workflow/WorkflowDefinition model. No local log
//! or caller-provided state is authoritative. Scope is exactly one graph.

use std::collections::{BTreeMap, BTreeSet};

use crate::{
    error::{Error, Result},
    namespace::{DEFAULT_BASE_NS, RDF_TYPE},
    store::{Datum, Store},
    types::Value,
};

type Node = BTreeMap<String, Vec<Value>>;

/// The pre-write workflow entities relevant to this transaction.
pub(crate) struct Before(BTreeMap<i64, Node>);

fn name(local: &str) -> String {
    format!("{DEFAULT_BASE_NS}{local}")
}

fn denied(message: impl Into<String>) -> Error {
    Error::PolicyDenied(format!("workflow: {}", message.into()))
}

fn read(store: &Store, entity: i64, graph: i64) -> Result<Node> {
    let mut stmt = store.prepare(
        "SELECT DISTINCT a,v FROM facts WHERE e=?1 AND g=?2 AND op=1 \
         AND valid_to IS NULL ORDER BY a,v",
    )?;
    let rows = stmt
        .query_map(rusqlite::params![entity, graph], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, Vec<u8>>(1)?))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let mut node = Node::new();
    for (attr, bytes) in rows {
        node.entry(store.resolve(attr)?)
            .or_default()
            .push(Value::from_bytes(&bytes)?);
    }
    Ok(node)
}

fn values<'a>(node: &'a Node, local: &str) -> &'a [Value] {
    node.get(&name(local)).map_or(&[], Vec::as_slice)
}

fn typed(store: &Store, node: &Node, class: &str) -> bool {
    node.get(RDF_TYPE).is_some_and(|vs| {
        vs.iter().any(|v| {
            matches!(v, Value::Ref(id) if store.resolve(*id).is_ok_and(|iri| iri == name(class)))
        })
    })
}

fn definition(store: &Store, node: &Node) -> bool {
    typed(store, node, "Workflow") || typed(store, node, "WorkflowDefinition")
}

fn one_ref(node: &Node, local: &str) -> Result<i64> {
    match values(node, local) {
        [Value::Ref(id)] => Ok(*id),
        _ => Err(denied(format!("{local} must have exactly one IRI"))),
    }
}

fn string(value: &Value) -> Option<&str> {
    match value {
        Value::Str(s) => Some(s),
        Value::Typed { lexical, datatype }
            if datatype == "http://www.w3.org/2001/XMLSchema#string" =>
        {
            Some(lexical)
        }
        _ => None,
    }
}

fn one_string<'a>(node: &'a Node, local: &str) -> Result<&'a str> {
    match values(node, local) {
        [v] => string(v)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| denied(format!("{local} must be a non-empty string"))),
        _ => Err(denied(format!("{local} must have exactly one string"))),
    }
}

fn one_integer(node: &Node, local: &str) -> Result<i64> {
    match values(node, local) {
        [Value::Int(n)] if *n >= 0 => Ok(*n),
        [Value::Typed { lexical, datatype }]
            if datatype == "http://www.w3.org/2001/XMLSchema#integer" =>
        {
            lexical
                .parse::<i64>()
                .ok()
                .filter(|n| *n >= 0 && n.to_string() == *lexical)
                .ok_or_else(|| denied(format!("{local} must be a nonnegative canonical integer")))
        }
        _ => Err(denied(format!(
            "{local} must have exactly one nonnegative integer"
        ))),
    }
}

fn linked_runs(node: &Node) -> impl Iterator<Item = i64> + '_ {
    values(node, "inRun").iter().filter_map(|v| match v {
        Value::Ref(id) => Some(*id),
        _ => None,
    })
}

/// Snapshot before staging; no production scans or persistent state cache.
pub(crate) fn capture(store: &Store, datums: &[Datum], graph: i64) -> Result<Before> {
    let mut ids = BTreeSet::new();
    let relevant = [
        "currentRevision",
        "fromRevision",
        "transitionVersion",
        "initialState",
        "terminalState",
        "hasStep",
        "fromState",
        "toState",
        "runOf",
        "currentState",
        "inRun",
        "atStep",
        "performedBy",
        "signature",
    ];
    for d in datums {
        let attr = store.resolve(d.attribute)?;
        let is_type = attr == RDF_TYPE
            && matches!(&d.value, Value::Ref(id) if store.resolve(*id).is_ok_and(|v|
                ["Workflow", "WorkflowDefinition", "WorkflowRun", "WorkflowStep", "TransitionEvent"]
                .iter().any(|kind| v == name(kind))));
        if is_type || relevant.iter().any(|a| attr == name(a)) {
            ids.insert(d.entity);
        }
        if attr == name("inRun")
            && let Value::Ref(id) = d.value
        {
            ids.insert(id);
        }
    }
    // Existing typed subjects must be protected even when only metadata or
    // a timestamp is touched. This consults touched subjects, never a graph scan.
    for class in [
        "Workflow",
        "WorkflowDefinition",
        "WorkflowRun",
        "WorkflowStep",
        "TransitionEvent",
    ] {
        for iri in super::placement::touched_of_type(store, datums, graph, class)? {
            if let Some(id) = store.lookup(&iri)? {
                ids.insert(id);
            }
        }
    }
    let mut nodes = BTreeMap::new();
    for id in ids {
        let node = read(store, id, graph)?;
        for run in linked_runs(&node) {
            if let std::collections::btree_map::Entry::Vacant(entry) = nodes.entry(run) {
                entry.insert(read(store, run, graph)?);
            }
        }
        nodes.insert(id, node);
    }
    Ok(Before(nodes))
}

fn check_definition(store: &Store, node: &Node, graph: i64) -> Result<()> {
    if !typed(store, node, "Workflow") || !typed(store, node, "WorkflowDefinition") {
        return Err(denied(
            "runOf must name one entity typed Workflow and WorkflowDefinition in this graph",
        ));
    }
    one_string(node, "initialState")?;
    let terminal = values(node, "terminalState");
    if terminal.is_empty() || terminal.iter().any(|v| string(v).is_none_or(str::is_empty)) {
        return Err(denied("workflow requires non-empty terminalState strings"));
    }
    if values(node, "hasStep").is_empty() {
        return Err(denied("workflow requires declared steps"));
    }
    let mut seen = BTreeSet::new();
    for v in values(node, "hasStep") {
        let Value::Ref(id) = v else {
            return Err(denied("hasStep must be an IRI"));
        };
        let step = read(store, *id, graph)?;
        if !typed(store, &step, "WorkflowStep") || !typed(store, &step, "Step") {
            return Err(denied(
                "hasStep must name one entity typed Step and WorkflowStep in this graph",
            ));
        }
        let from = one_string(&step, "fromState")?;
        one_string(&step, "toState")?;
        let iri = store.resolve(*id)?;
        if !seen.insert((
            from.to_string(),
            iri.rsplit('/').next().unwrap_or(&iri).to_string(),
        )) {
            return Err(denied(
                "step name and fromState must identify one declared transition",
            ));
        }
        if terminal.iter().any(|v| string(v) == Some(from)) {
            return Err(denied("a transition cannot leave a terminal state"));
        }
    }
    Ok(())
}

/// Validate the pending post-state against the captured current state.
pub(crate) fn validate(store: &Store, before: &Before, datums: &[Datum], graph: i64) -> Result<()> {
    // Enrolled processing requires authenticity even if the separate legacy
    // signature flag is off. The canonical signing protocol stays unchanged.
    super::transition::verify_write(store, datums, graph)?;
    let mut new_events = BTreeMap::new();
    for (id, prior) in &before.0 {
        let post = read(store, *id, graph)?;
        if typed(store, prior, "TransitionEvent") {
            if prior != &post {
                return Err(denied("committed transition events are immutable"));
            }
        } else if typed(store, &post, "TransitionEvent") {
            if one_integer(&post, "transitionVersion")? != 2 {
                return Err(denied(
                    "enforced workflows require revision-bound v2 signatures",
                ));
            }
            let run = one_ref(&post, "inRun")?;
            if new_events.insert(run, post).is_some() {
                return Err(denied("one advance per run per transaction"));
            }
        } else if definition(store, prior) || typed(store, prior, "WorkflowStep") {
            let keys = [
                "initialState",
                "terminalState",
                "hasStep",
                "fromState",
                "toState",
            ];
            if keys
                .iter()
                .any(|key| values(prior, key) != values(&post, key))
                || (definition(store, prior) && !definition(store, &post))
                || (typed(store, prior, "WorkflowStep") && !typed(store, &post, "WorkflowStep"))
            {
                return Err(denied(
                    "committed workflow plans are immutable; define a new identity",
                ));
            }
        } else if definition(store, &post) {
            check_definition(store, &post, graph)?;
        }
    }
    for (id, prior) in &before.0 {
        let post = read(store, *id, graph)?;
        let was_run = typed(store, prior, "WorkflowRun");
        if !was_run && !typed(store, &post, "WorkflowRun") {
            continue;
        }
        if !typed(store, &post, "WorkflowRun") {
            return Err(denied("a committed run cannot lose its type"));
        }
        let wf = one_ref(&post, "runOf")?;
        let plan = read(store, wf, graph)?;
        check_definition(store, &plan, graph)?;
        let current = one_string(&post, "currentState")?;
        let revision = one_integer(&post, "currentRevision")?;
        if !was_run {
            if new_events.contains_key(id)
                || revision != 0
                || current != one_string(&plan, "initialState")?
            {
                return Err(denied("new runs must start at the declared initial state"));
            }
            continue;
        }
        if one_ref(prior, "runOf")? != wf {
            return Err(denied("runOf is immutable"));
        }
        let previous = one_string(prior, "currentState")?;
        let previous_revision = one_integer(prior, "currentRevision")?;
        if let Some(event) = new_events.remove(id) {
            if one_integer(&event, "fromRevision")? != previous_revision
                || previous_revision.checked_add(1) != Some(revision)
            {
                return Err(denied(
                    "advance must consume the current revision exactly once",
                ));
            }
            let step_id = one_ref(&event, "atStep")?;
            if !values(&plan, "hasStep").contains(&Value::Ref(step_id)) {
                return Err(denied("event step is not declared by this run's workflow"));
            }
            let step = read(store, step_id, graph)?;
            if one_string(&event, "fromState")? != previous
                || one_string(&step, "fromState")? != previous
                || one_string(&event, "toState")? != current
                || one_string(&step, "toState")? != current
            {
                return Err(denied(
                    "event must perform the declared transition from current state",
                ));
            }
        } else if current != previous || revision != previous_revision {
            return Err(denied(
                "state changes require a new signed transition in the same write",
            ));
        }
    }
    if !new_events.is_empty() {
        return Err(denied(
            "a transition must reference an existing run in this graph",
        ));
    }
    Ok(())
}
