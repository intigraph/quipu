//! Narrow a typed named-graph scan by absence of a plain literal property.
//!
//! A type scan, optionally constrained by fixed same-subject properties,
//! can reject subjects with a plain property before decoding every binding.
//! The SQL anti-join drops only subjects with a proven plain string value.
//! The original FILTER still evaluates every surviving binding.
//!
//! Root/default graphs, attachments, historical/seeded reads and every other
//! expression keep the exact existing evaluator, including type entailment.

use rusqlite::functions::FunctionFlags;
use spargebra::algebra::{Expression, Function, GraphPattern};
use spargebra::term::{NamedNodePattern, TermPattern, TriplePattern};

use super::pattern_util::Bindings;
use super::{GraphScope, TemporalContext};
use crate::error::Result;
use crate::store::Store;
use crate::types::Value;

const RDF_TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";

struct Plan<'a> {
    subject: &'a str,
    class: &'a str,
    property: &'a str,
    constraints: Vec<&'a TriplePattern>,
}

fn variable(expr: &Expression, name: &str) -> bool {
    matches!(expr, Expression::Variable(v) if v.as_str() == name)
}

fn str_of(expr: &Expression, name: &str) -> bool {
    matches!(expr, Expression::FunctionCall(Function::Str, args)
        if args.len() == 1 && variable(&args[0], name))
}

fn is_literal(expr: &Expression, name: &str) -> bool {
    matches!(expr, Expression::FunctionCall(Function::IsLiteral, args)
        if args.len() == 1 && variable(&args[0], name))
}

fn identity_string(expr: &Expression, name: &str) -> bool {
    matches!(expr, Expression::SameTerm(a, b)
        if (variable(a, name) && str_of(b, name))
        || (variable(b, name) && str_of(a, name)))
}

fn plain_guard(expr: &Expression, name: &str) -> bool {
    matches!(expr, Expression::And(a, b)
        if (is_literal(a, name) && identity_string(b, name))
        || (is_literal(b, name) && identity_string(a, name)))
}

fn plan<'a>(expr: &'a Expression, inner: &'a GraphPattern) -> Option<Plan<'a>> {
    let GraphPattern::Bgp { patterns } = inner else {
        return None;
    };
    if patterns.is_empty() || patterns.len() > 8 {
        return None;
    }
    let typed = patterns.iter().find(|tp| {
        matches!((&tp.subject, &tp.predicate, &tp.object),
            (TermPattern::Variable(_), NamedNodePattern::NamedNode(p), TermPattern::NamedNode(_))
            if p.as_str() == RDF_TYPE)
    })?;
    if patterns.iter().any(|tp| {
        tp.subject != typed.subject
            || !matches!(tp.predicate, NamedNodePattern::NamedNode(_))
            || !matches!(
                tp.object,
                TermPattern::NamedNode(_) | TermPattern::Literal(_)
            )
    }) {
        return None;
    }
    let (
        TermPattern::Variable(subject),
        NamedNodePattern::NamedNode(predicate),
        TermPattern::NamedNode(class),
    ) = (&typed.subject, &typed.predicate, &typed.object)
    else {
        return None;
    };
    if predicate.as_str() != RDF_TYPE {
        return None;
    }
    let constraints = patterns.iter().filter(|tp| *tp != typed).collect();
    let Expression::Not(negated) = expr else {
        return None;
    };
    let Expression::Exists(exists) = negated.as_ref() else {
        return None;
    };
    let GraphPattern::Filter {
        expr: guard,
        inner: property_pattern,
    } = exists.as_ref()
    else {
        return None;
    };
    let GraphPattern::Bgp { patterns } = property_pattern.as_ref() else {
        return None;
    };
    let [property] = patterns.as_slice() else {
        return None;
    };
    let (
        TermPattern::Variable(other_subject),
        NamedNodePattern::NamedNode(property_name),
        TermPattern::Variable(value),
    ) = (&property.subject, &property.predicate, &property.object)
    else {
        return None;
    };
    if other_subject != subject || value == subject || !plain_guard(guard, value.as_str()) {
        return None;
    }
    Some(Plan {
        subject: subject.as_str(),
        class: class.as_str(),
        property: property_name.as_str(),
        constraints,
    })
}

fn constraint_sql(
    store: &Store,
    tp: &TriplePattern,
    params: &mut Vec<Box<dyn rusqlite::types::ToSql>>,
) -> Result<Option<String>> {
    let NamedNodePattern::NamedNode(predicate) = &tp.predicate else {
        return Ok(None);
    };
    let ids = store.lookup_all(predicate.as_str())?;
    let [predicate] = ids.as_slice() else {
        return Ok(None); // Unknown or aliased ids retain the ordinary evaluator.
    };
    let Some(value) =
        super::pattern_util::resolve_object_pattern(store, &tp.object, &Bindings::new())?
    else {
        return Ok(None);
    };
    let values = if let Value::Ref(id) = value {
        if id < 0 || store.lookup_all(&store.resolve(id)?)?.len() != 1 {
            return Ok(None);
        }
        vec![Value::Ref(id).to_bytes()]
    } else {
        store.literal_aliases(&value, true)?
    };
    let predicate_param = params.len() + 1;
    params.push(Box::new(*predicate));
    let mut placeholders = Vec::new();
    for value in values {
        placeholders.push(format!("?{}", params.len() + 1));
        params.push(Box::new(value));
    }
    if placeholders.is_empty() {
        return Ok(None);
    }
    Ok(Some(format!(
        " AND EXISTS (SELECT 1 FROM facts AS c INDEXED BY idx_current_aev
            WHERE c.e=t.e AND c.a=?{predicate_param} AND c.v IN ({})
            AND c.op=1 AND c.valid_to IS NULL AND EXISTS
                (SELECT 1 FROM facts AS cg INDEXED BY idx_current_g
                    WHERE cg.g=?3 AND cg.rowid=c.rowid
                    AND cg.op=1 AND cg.valid_to IS NULL))",
        placeholders.join(",")
    )))
}

/// Register the packed-value predicate on writer and pooled read connections.
///
/// # Errors
/// Returns the SQLite registration error if the function cannot be installed.
pub fn register(conn: &rusqlite::Connection) -> rusqlite::Result<()> {
    conn.create_scalar_function(
        "quipu_plain_literal",
        1,
        FunctionFlags::SQLITE_UTF8
            | FunctionFlags::SQLITE_INNOCUOUS
            | FunctionFlags::SQLITE_DETERMINISTIC,
        |ctx| {
            let bytes: Vec<u8> = ctx.get(0)?;
            // A malformed value cannot prove the property present: retain its
            // subject for the ordinary decoder/FILTER to decide or fail.
            Ok(matches!(Value::from_bytes(&bytes), Ok(Value::Str(_))))
        },
    )
}

/// Candidates for a proven absence filter, or None for the original evaluator.
///
/// # Errors
/// Propagates dictionary, SQL and query-budget failures without partial output.
pub fn candidates(
    store: &Store,
    expr: &Expression,
    inner: &GraphPattern,
    ctx: &TemporalContext,
    seed: &Bindings,
) -> Result<Option<(Vec<Bindings>, Vec<String>)>> {
    if !seed.is_empty()
        || !store.attachments().is_empty()
        || ctx.valid_at.is_some()
        || ctx.as_of_tx.is_some()
        || ctx.row_limit.is_some()
        || ctx.string_narrows.is_some()
    {
        return Ok(None);
    }
    let GraphScope::Named(graphs) = &ctx.graph else {
        return Ok(None);
    };
    let [graph] = graphs.as_slice() else {
        return Ok(None);
    };
    if *graph == 0 {
        return Ok(None);
    }
    let Some(plan) = plan(expr, inner) else {
        return Ok(None);
    };
    let (Some(rdf_type), Some(class), Some(property)) = (
        store.lookup(RDF_TYPE)?,
        store.lookup(plan.class)?,
        store.lookup(plan.property)?,
    ) else {
        return Ok(None);
    };
    // No composition, so ids are canonical and no cross-layer anti-join is
    // needed. Named graph scope was already admitted by the GRAPH evaluator.
    // Intersect existing covering indexes by ROWID of the SAME fact. This
    // keeps graph/currentness checks off the large fact table pages. Readers
    // without the indexes retain the exact ordinary evaluator.
    let indexes: i64 = store.conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name IN
            ('idx_active_vge','idx_current_aev','idx_current_g')",
        [],
        |r| r.get(0),
    )?;
    if indexes != 3 {
        return Ok(None);
    }
    let mut parameters: Vec<Box<dyn rusqlite::types::ToSql>> = vec![
        Box::new(rdf_type),
        Box::new(Value::Ref(class).to_bytes()),
        Box::new(*graph),
        Box::new(property),
    ];
    let mut sql = String::from(
        "SELECT DISTINCT t.e FROM facts AS t INDEXED BY idx_active_vge
        WHERE t.v=?2 AND t.g=?3 AND t.op=1 AND t.valid_to IS NULL
        AND EXISTS (SELECT 1 FROM facts AS typed INDEXED BY idx_current_aev
            WHERE typed.a=?1 AND typed.e=t.e AND typed.v=?2
            AND typed.rowid=t.rowid AND typed.op=1 AND typed.valid_to IS NULL)
        AND NOT EXISTS (SELECT 1 FROM facts AS p INDEXED BY idx_current_aev
            WHERE p.e=t.e AND p.a=?4 AND p.op=1 AND p.valid_to IS NULL
            AND quipu_plain_literal(p.v) AND EXISTS
                (SELECT 1 FROM facts AS gp INDEXED BY idx_current_g
                    WHERE gp.g=?3 AND gp.rowid=p.rowid
                    AND gp.op=1 AND gp.valid_to IS NULL))",
    );
    for tp in &plan.constraints {
        let Some(constraint) = constraint_sql(store, tp, &mut parameters)? else {
            return Ok(None);
        };
        sql.push_str(&constraint);
    }
    let mut stmt = store.prepare(&sql)?;
    let mut rows = stmt.query(rusqlite::params_from_iter(
        parameters.iter().map(std::convert::AsRef::as_ref),
    ))?;
    let mut bindings = Vec::new();
    while let Some(row) = rows.next()? {
        super::pattern_util::check_eval_budget(ctx, bindings.len(), bindings.len())?;
        let subject: i64 = row.get(0)?;
        let mut binding = Bindings::new();
        binding.insert(plan.subject.to_string(), Value::Ref(subject));
        bindings.push(binding);
    }
    Ok(Some((bindings, vec![plan.subject.to_string()])))
}
