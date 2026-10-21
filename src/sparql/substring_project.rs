//! Subject-only projection from an explicitly complete local substring index.
//! Exact trigram phrases decide ordinary text; NUL normalization still needs
//! lexical verification. Fingerprints never decide RDF equality.
use super::{Bindings, TemporalContext};
use crate::{Result, Store, Value};
use spargebra::algebra::{Expression, Function, GraphPattern};
use spargebra::term::{NamedNodePattern, TermPattern};
use std::collections::HashSet;
fn variable(e: &Expression, name: &str) -> bool {
    matches!(e,Expression::Variable(v) if v.as_str()==name)
}
fn call(e: &Expression, function: &Function, name: &str) -> bool {
    matches!(e,Expression::FunctionCall(f,args) if f==function&&args.len()==1&&variable(&args[0],name))
}
fn str_var(e: &Expression, name: &str) -> bool {
    call(e, &Function::Str, name)
}
fn conjuncts<'a>(e: &'a Expression, out: &mut Vec<&'a Expression>) {
    if let Expression::And(a, b) = e {
        conjuncts(a, out);
        conjuncts(b, out);
    } else {
        out.push(e);
    }
}
fn needle<'a>(expr: &'a Expression, name: &str) -> Option<&'a str> {
    let mut parts = Vec::new();
    conjuncts(expr, &mut parts);
    if parts.len() != 3 {
        return None;
    }
    let (mut literal, mut plain, mut value) = (false, false, None);
    for p in parts {
        if call(p, &Function::IsLiteral, name) {
            if literal {
                return None;
            }
            literal = true;
        } else if matches!(p,Expression::SameTerm(a,b) if (variable(a,name)&&str_var(b,name))||(variable(b,name)&&str_var(a,name)))
        {
            if plain {
                return None;
            }
            plain = true;
        } else if let Expression::FunctionCall(Function::Contains, args) = p {
            if value.is_some() || args.len() != 2 {
                return None;
            }
            let Expression::FunctionCall(Function::LCase, folded) = &args[0] else {
                return None;
            };
            if folded.len() != 1 || !str_var(&folded[0], name) {
                return None;
            }
            let Expression::Literal(text) = &args[1] else {
                return None;
            };
            if text.datatype().as_str() != "http://www.w3.org/2001/XMLSchema#string" {
                return None;
            }
            value = Some(text.value());
        } else {
            return None;
        }
    }
    let value = value?;
    (literal && plain && !value.contains('\0') && value.chars().count() >= 3).then_some(value)
}
fn term_key(store: &Store, rowid: i64) -> Result<Vec<u8>> {
    let bytes: Vec<u8> =
        store
            .conn
            .query_row("SELECT v FROM main.facts WHERE rowid=?1", [rowid], |r| {
                r.get(0)
            })?;
    Ok(Value::from_bytes(&bytes)?.term_key())
}
/// Unsupported shapes retain the ordinary evaluator; query errors propagate.
pub(super) fn try_eval(
    store: &Store,
    inner: &GraphPattern,
    vars: &[String],
    ctx: &TemporalContext,
    seed: &Bindings,
) -> Result<Option<(Vec<Bindings>, Vec<String>)>> {
    let GraphPattern::Filter { expr, inner } = inner else {
        return Ok(None);
    };
    let GraphPattern::Bgp { patterns } = inner.as_ref() else {
        return Ok(None);
    };
    let [triple] = patterns.as_slice() else {
        return Ok(None);
    };
    let (
        TermPattern::Variable(subject),
        NamedNodePattern::NamedNode(predicate),
        TermPattern::Variable(object),
    ) = (&triple.subject, &triple.predicate, &triple.object)
    else {
        return Ok(None);
    };
    let name = subject.as_str();
    if vars.len() != 1
        || vars[0] != name
        || name == object.as_str()
        || ctx.row_limit.is_some()
        || store.has_attachments()
        || ctx.valid_at.is_some()
        || ctx.as_of_tx.is_some()
    {
        return Ok(None);
    }
    let Some(needle) = needle(expr, object.as_str()) else {
        return Ok(None);
    };
    let Some(graph) = ctx.graph.single_graph().filter(|g| *g >= 0) else {
        return Ok(None);
    };
    if !store.substring_index_ready()? {
        return Ok(None);
    }
    let predicates = store.lookup_all(predicate.as_str())?;
    let [predicate] = predicates.as_slice() else {
        return Ok(None);
    };
    // Exact compact g/a checks below establish scope. Do not intersect a
    // redundant high-frequency FTS scope posting list for every text leaf.
    let phrase = format!("text:\"{}\"", needle.replace('"', "\"\""));
    let mut params: Vec<rusqlite::types::Value> = vec![
        phrase.into(),
        needle.to_owned().into(),
        graph.into(),
        (*predicate).into(),
    ];
    // The exact readiness DDL fixes case_sensitive=1 trigram semantics, and
    // needle() requires >=3 Unicode characters without NUL. Ordinary stored
    // text is already lowercase: the quoted phrase proves the substring.
    // Normalized NUL text alone may have false positives and must be checked.
    // The transaction-maintained compact flag avoids even opening an ordinary
    // FTS content row just to discover that exact_text is NULL.
    let mut conditions = vec![
        "substring_fts MATCH ?1".to_owned(),
        "(f.needs_exact=0 OR instr(substring_fts.exact_text,?2)>0)".into(),
        "f.g=?3".into(),
        "f.a=?4".into(),
        "f.plain=1".into(),
        "f.fact_id=substring_fts.rowid".into(),
    ];
    if let Some(value) = seed.get(name) {
        let Value::Ref(id) = value else {
            return Ok(None);
        };
        if *id < 0 {
            return Ok(None);
        }
        params.push((*id).into());
        conditions.push(format!("f.e=?{}", params.len()));
    }
    super::pattern_util::check_eval_budget(ctx, 0, 0)?;
    let sql = format!(
        "SELECT f.e,f.value_hash,f.fact_id FROM main.substring_fts CROSS JOIN main.substring_facts AS f NOT INDEXED WHERE {} ORDER BY f.e,f.value_hash",
        conditions.join(" AND ")
    );
    let mut statement = store.conn.prepare(&sql)?;
    let mut cursor = statement.query(rusqlite::params_from_iter(params))?;
    let mut output = Vec::new();
    let mut previous: Option<(i64, Vec<u8>, i64)> = None;
    let mut exact = HashSet::new();
    let mut visited = 0;
    while let Some(row) = cursor.next()? {
        super::pattern_util::check_eval_budget(ctx, visited, output.len())?;
        visited += 1;
        let subject: i64 = row.get(0)?;
        let hash: Vec<u8> = row.get(1)?;
        let rowid: i64 = row.get(2)?;
        let emit = if let Some((old_subject, old_hash, first)) = &previous {
            if *old_subject == subject && *old_hash == hash {
                // Even a forced fingerprint collision cannot collapse terms.
                if exact.is_empty() {
                    exact.insert(term_key(store, *first)?);
                }
                exact.insert(term_key(store, rowid)?)
            } else {
                exact.clear();
                true
            }
        } else {
            true
        };
        let same = previous
            .as_ref()
            .is_some_and(|(s, h, _)| *s == subject && *h == hash);
        if !same {
            previous = Some((subject, hash, rowid));
        }
        if emit {
            output.push(Bindings::from([(
                name.to_owned(),
                Value::Ref(store.canonical_id(subject)?),
            )]));
        }
    }
    super::pattern_util::check_eval_budget(ctx, visited, output.len())?;
    Ok(Some((output, vars.to_vec())))
}
