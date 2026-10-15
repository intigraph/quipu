//! Bind join for `VALUES` (aegis-roth88).
//!
//! The ordinary `Join` arm evaluates both operands independently under the same
//! seed and hash-joins the results. For `VALUES ?s { … } GRAPH <g> { ?s ?p ?o }`
//! that evaluates `?s ?p ?o` with `?s` UNBOUND — a scan of the whole graph — and
//! only the join throws the rest away. Measured on the full seeds board
//! (726,012 triples): 40 subjects through `VALUES` took 3.4–5.3 s, while the
//! same 40 as a `UNION` of bound patterns took 0.007 s, with identical rows.
//!
//! A bind join instead evaluates the other operand once per `VALUES` row,
//! SEEDED with that row, so a bound `?s` reaches the leaf as a subject lookup.
//!
//! Seeding is substitution, and substitution only equals join semantics for
//! operands that cannot observe a variable before it is joined: BGPs, `GRAPH`
//! around them, and joins of those. `OPTIONAL` with a filter, `MINUS`, `FILTER`,
//! subqueries and paths can — a filter that sees `?x` bound by substitution
//! answers differently from one evaluated bottom-up — so they keep the hash join.

use spargebra::algebra::GraphPattern;
use spargebra::term::{GroundTerm, NamedNodePattern, TermPattern, TriplePattern, Variable};

use crate::error::Result;
use crate::store::Store;

use super::pattern::eval_pattern_seeded;
use super::pattern_util::{check_eval_budget, triple_pattern_vars};
use super::values::eval_values;
use super::{Bindings, TemporalContext};

/// Above this many `VALUES` rows, one seeded evaluation per row stops being
/// obviously cheaper than one scan plus a hash join, so the hash join is kept.
pub(crate) const BIND_JOIN_MAX_ROWS: usize = 1024;

/// Whether substituting bindings into `pattern` is the same as joining with it.
pub(crate) fn seed_safe(pattern: &GraphPattern) -> bool {
    match pattern {
        GraphPattern::Bgp { .. } => true,
        GraphPattern::Graph { inner, .. } => seed_safe(inner),
        GraphPattern::Join { left, right } => seed_safe(left) && seed_safe(right),
        _ => false,
    }
}

/// The variables `pattern` binds, in the order its evaluation reports them.
/// Only called on [`seed_safe`] patterns; needed when the `VALUES` table is
/// empty and nothing is evaluated, so the projection header is unchanged.
fn seed_safe_vars(pattern: &GraphPattern) -> Vec<String> {
    let mut vars: Vec<String> = Vec::new();
    match pattern {
        GraphPattern::Bgp { patterns } => {
            for tp in patterns {
                for v in triple_pattern_vars(tp) {
                    push_unique(&mut vars, v);
                }
            }
        }
        GraphPattern::Graph { name, inner } => {
            for v in seed_safe_vars(inner) {
                push_unique(&mut vars, v);
            }
            if let NamedNodePattern::Variable(g) = name {
                push_unique(&mut vars, g.as_str().to_string());
            }
        }
        GraphPattern::Join { left, right } => {
            for v in seed_safe_vars(left)
                .into_iter()
                .chain(seed_safe_vars(right))
            {
                push_unique(&mut vars, v);
            }
        }
        _ => {}
    }
    vars
}

fn push_unique(vars: &mut Vec<String>, v: String) {
    if !vars.contains(&v) {
        vars.push(v);
    }
}

/// The `VALUES` table of a join operand, when that operand is one.
type ValuesTable<'a> = (&'a [Variable], &'a [Vec<Option<GroundTerm>>]);

fn as_values(pattern: &GraphPattern) -> Option<ValuesTable<'_>> {
    match pattern {
        GraphPattern::Values {
            variables,
            bindings,
        } => Some((variables, bindings)),
        _ => None,
    }
}

/// Evaluate `left JOIN right` as a bind join when one side is a small `VALUES`
/// table and the other is [`seed_safe`]. `None` means "not applicable": the
/// caller keeps its hash join, so this can only ever narrow evaluation.
pub(crate) fn try_values_bind_join(
    store: &Store,
    left: &GraphPattern,
    right: &GraphPattern,
    ctx: &TemporalContext,
    seed: &Bindings,
) -> Result<Option<(Vec<Bindings>, Vec<String>)>> {
    let (values_left, (variables, bindings), other) = match (as_values(left), as_values(right)) {
        (Some(v), None) if seed_safe(right) => (true, v, right),
        (None, Some(v)) if seed_safe(left) => (false, v, left),
        _ => return Ok(None),
    };
    if bindings.len() > BIND_JOIN_MAX_ROWS {
        return Ok(None);
    }

    let (value_rows, value_vars) = eval_values(store, variables, bindings, seed)?;
    // Join operands never carry a pushed-down row cap (limit_pushdown_safe
    // rejects joins), but a per-row cap would be wrong here, so clear it.
    let row_ctx = TemporalContext {
        row_limit: None,
        ..ctx.clone()
    };
    let mut rows = Vec::new();
    let mut other_vars = None;
    for (i, row) in value_rows.iter().enumerate() {
        check_eval_budget(ctx, i, rows.len())?;
        // The seed already carries the VALUES row, so every result row is the
        // join of that row with a compatible solution of `other`.
        let (mut found, vars) = eval_pattern_seeded(store, other, &row_ctx, row)?;
        rows.append(&mut found);
        other_vars.get_or_insert(vars);
    }
    let other_vars = other_vars.unwrap_or_else(|| seed_safe_vars(other));

    // Header order matches the hash-join arm: left operand's variables first.
    let (first, second) = if values_left {
        (value_vars, other_vars)
    } else {
        (other_vars, value_vars)
    };
    let mut vars = first;
    for v in second {
        push_unique(&mut vars, v);
    }
    Ok(Some((rows, vars)))
}

/// A selected relation may drive indexed metadata lookups without scanning the
/// unrelated right relation. This is a finite work bound, not a result limit.
pub(crate) const SUBJECT_BIND_MAX_ROWS: usize = 16_384;

// These wrappers cannot observe substitution; variable GRAPH and expressions
// deliberately decline. Keep this separate from the broader VALUES planner.
fn subject_lookup_patterns<'a>(
    pattern: &'a GraphPattern,
    triples: &mut Vec<&'a TriplePattern>,
    variables: &mut Vec<String>,
) -> bool {
    match pattern {
        GraphPattern::Bgp { patterns } => {
            for triple in patterns {
                triples.push(triple);
                for variable in triple_pattern_vars(triple) {
                    push_unique(variables, variable);
                }
            }
            true
        }
        GraphPattern::Graph { name: NamedNodePattern::NamedNode(_), inner } =>
            subject_lookup_patterns(inner, triples, variables),
        GraphPattern::Union { left, right } =>
            subject_lookup_patterns(left, triples, variables)
                && subject_lookup_patterns(right, triples, variables),
        _ => false,
    }
}

/// Bind a plain BGP or constant GRAPH/UNION thereof to a left relation. Every right
/// subject must be constant or an IRI bound in every input row, and at least
/// one must be newly supplied by the left operand. FILTER, OPTIONAL, MINUS,
/// paths, variable GRAPH and subqueries keep bottom-up evaluation.
///
/// Only expression-free BGP substitution is used: no expression gains visibility of bindings
/// that it would not have in the ordinary join. Multiplicity is retained by
/// evaluating once for every left row, including duplicates.
pub(crate) fn try_subject_bind_join(
    store: &Store,
    right: &GraphPattern,
    ctx: &TemporalContext,
    seed: &Bindings,
    left_rows: &[Bindings],
    left_vars: &[String],
) -> Result<Option<(Vec<Bindings>, Vec<String>)>> {
    let mut patterns = Vec::new();
    let mut vars = Vec::new();
    if !subject_lookup_patterns(right, &mut patterns, &mut vars) {
        return Ok(None);
    }
    if left_rows.len() > SUBJECT_BIND_MAX_ROWS {
        return Ok(None);
    }
    if left_rows.is_empty() {
        return Ok(Some((Vec::new(), vars)));
    }
    let mut newly_bound_subject = false;
    for pattern in patterns {
        match &pattern.subject {
            TermPattern::NamedNode(_) => {}
            TermPattern::Variable(variable) => {
                let name = variable.as_str();
                for (i, row) in left_rows.iter().enumerate() {
                    // Preparation is inside the same deadline as execution.
                    check_eval_budget(ctx, i, 0)?;
                    if !matches!(row.get(name), Some(crate::types::Value::Ref(id)) if *id > 0) {
                        return Ok(None);
                    }
                }
                newly_bound_subject |=
                    !seed.contains_key(name) && left_vars.iter().any(|v| v == name);
            }
            _ => return Ok(None),
        }
    }
    if !newly_bound_subject {
        return Ok(None);
    }
    let row_ctx = TemporalContext {
        row_limit: None,
        ..ctx.clone()
    };
    let mut rows = Vec::new();
    for (i, row) in left_rows.iter().enumerate() {
        check_eval_budget(ctx, i, rows.len())?;
        let (mut found, _) = eval_pattern_seeded(store, right, &row_ctx, row)?;
        rows.append(&mut found);
        check_eval_budget(ctx, i, rows.len())?;
    }
    Ok(Some((rows, vars)))
}
