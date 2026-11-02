//! `VALUES` — inline relation evaluation (quipu #51).
//!
//! `VALUES` is a literal table of solutions written into the query. SPARQL
//! defines it as a multiset of solution mappings that joins with the rest of
//! the group, so it is evaluated as a leaf (like a BGP): each declared row is
//! merged with the seed bindings, and rows that conflict with the seed drop
//! out. Everything else — the join with a neighbouring BGP, in either order —
//! falls out of the ordinary `Join` arm.

use spargebra::term::{GroundTerm, Variable};

use crate::error::Result;
use crate::store::Store;
use crate::types::Value;

use super::Bindings;

/// Evaluate a `VALUES` block into rows plus the variables it declares.
///
/// Semantics that are easy to get wrong, and are covered by tests:
///
/// - `UNDEF` leaves the variable *unbound* in that row rather than binding it
///   to a sentinel — a later `OPTIONAL`/`BOUND` must still see it as unbound.
/// - An empty table (`VALUES ?x {}`) yields **zero** rows, not every row. A
///   `VALUES` that constrains nothing would be a silent no-op filter (cf. #12),
///   which is exactly the failure mode this repo treats as worse than an error.
/// - The declared variables are returned even when the table is empty, so
///   projection still knows about them.
pub fn eval_values(
    store: &Store,
    variables: &[Variable],
    bindings: &[Vec<Option<GroundTerm>>],
    seed: &Bindings,
) -> Result<(Vec<Bindings>, Vec<String>)> {
    let vars: Vec<String> = variables.iter().map(|v| v.as_str().to_string()).collect();

    let mut rows = Vec::with_capacity(bindings.len());
    'row: for binding in bindings {
        let mut row = seed.clone();
        for (var, term) in vars.iter().zip(binding) {
            // UNDEF: no binding for this variable in this row.
            let Some(term) = term else { continue };
            let value = ground_term_to_value(store, term)?;
            // A row that disagrees with the seed (EXISTS substitution, or a
            // value already fixed by an enclosing pattern) is not a solution.
            match row.get(var) {
                Some(existing) if existing != &value => continue 'row,
                Some(_) => {}
                None => {
                    row.insert(var.clone(), value);
                }
            }
        }
        rows.push(row);
    }

    Ok((rows, vars))
}

/// The inline table's rows as values, converted once (aegis-xdvg.6). `None`
/// is UNDEF.
pub(super) fn values_table(
    store: &Store,
    bindings: &[Vec<Option<GroundTerm>>],
) -> Result<Vec<Vec<Option<Value>>>> {
    bindings
        .iter()
        .map(|row| {
            row.iter()
                .map(|term| {
                    term.as_ref()
                        .map(|t| ground_term_to_value(store, t))
                        .transpose()
                })
                .collect()
        })
        .collect()
}

/// Join `row` with each table row in place (aegis-xdvg.6): bind what the row
/// lacks, skip a table row that disagrees with a bound variable (as
/// [`eval_values`] does), call `emit`, then restore the row.
pub(super) fn join_row(
    variables: &[Variable],
    table: &[Vec<Option<Value>>],
    row: &mut Bindings,
    emit: &mut dyn FnMut(&mut Bindings) -> Result<()>,
) -> Result<()> {
    let mut added: Vec<&str> = Vec::with_capacity(variables.len());
    'row: for values in table {
        added.clear();
        for (variable, value) in variables.iter().zip(values) {
            let Some(value) = value else { continue };
            match row.get(variable.as_str()) {
                Some(existing) if existing != value => {
                    for name in &added {
                        row.remove(*name);
                    }
                    continue 'row;
                }
                Some(_) => {}
                None => {
                    row.insert(variable.as_str().to_owned(), value.clone());
                    added.push(variable.as_str());
                }
            }
        }
        let result = emit(row);
        for name in &added {
            row.remove(*name);
        }
        result?;
    }
    Ok(())
}

/// Convert a `VALUES` ground term into the store's `Value` representation.
///
/// An IRI resolves to the interned `Ref` the fact log uses, so it joins with a
/// BGP binding for the same IRI. An IRI absent from the dictionary keeps its
/// lexical form (`Str`) — the same fallback the BGP subject binding uses. That
/// matters: a sentinel id would make two *different* unknown IRIs compare
/// equal, while `Str` stays distinct and still matches no fact.
fn ground_term_to_value(store: &Store, term: &GroundTerm) -> Result<Value> {
    Ok(match term {
        // `lookup_all` puts the local id first, then any attached layer's id, so
        // an IRI known only to a layer still joins (aegis-gwu); unattached stores
        // see exactly `lookup`.
        GroundTerm::NamedNode(n) => store
            .lookup_all(n.as_str())?
            .first()
            .copied()
            .map_or_else(|| Value::Str(n.as_str().to_string()), Value::Ref),
        GroundTerm::Literal(lit) => super::filter::literal_to_value(lit),
        // Quoted triples (RDF-star) are not stored as values, so one can never
        // match. Bind the lexical form rather than erroring: the row is then
        // simply unsatisfiable, which is what the query asked for.
        #[cfg(feature = "shacl")]
        GroundTerm::Triple(t) => Value::Str(format!("{t}")),
    })
}
