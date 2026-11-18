//! Actual ROOT history and receipt-bound queries, never expected-state replay.
use super::*;
use quipu::sparql::{QueryResult, TemporalContext, query_temporal};
use rusqlite::{Connection, OpenFlags, params};
use std::path::Path;

fn string(value: Value) -> Res<String> {
    match value {
        Value::Str(s) => Ok(s),
        _ => Err("fixture read returned non-string".into()),
    }
}
fn query(store: &Store, tx: Option<i64>, valid_at: Option<String>) -> Res<Vec<String>> {
    let ctx = TemporalContext {
        as_of_tx: tx,
        valid_at,
        ..Default::default()
    };
    let QueryResult::Select { rows, .. } = query_temporal(
        store,
        &format!("SELECT ?v WHERE {{ <{SUBJECT}> <{PREDICATE}> ?v }} LIMIT 4097"),
        &ctx,
    )?
    else {
        return Err("native read did not return SELECT".into());
    };
    let mut values = rows
        .into_iter()
        .map(|r| string(r.get("v").ok_or("missing binding")?.clone()))
        .collect::<Res<Vec<_>>>()?;
    if values.len() > 4096 {
        return Err("query capture exceeds bound".into());
    }
    values.sort();
    Ok(values)
}

fn receipt_role(source: &str) -> Res<&'static str> {
    let prefix = format!("fork-promote:{FORK}");
    if source == prefix {
        return Ok("receive");
    }
    let (body, role) = source
        .strip_suffix(";step=decide")
        .map_or((source, "receive"), |body| (body, "decide"));
    let rest = body
        .strip_prefix(&format!("{prefix};decisions="))
        .ok_or("unrecognized native receipt prefix")?;
    let hash = rest
        .strip_suffix(";reviewer=oracle-input")
        .and_then(|s| s.strip_prefix("sha256:"))
        .ok_or("unrecognized native receipt binding")?;
    if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("unrecognized native receipt hash".into());
    }
    Ok(role)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn capture(
    store: &Store,
    db: &Path,
    e: i64,
    a: i64,
    before: i64,
    after: i64,
    base_tx: i64,
    probes: &[Probe],
) -> Res<serde_json::Value> {
    let conn = Connection::open_with_flags(db, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let mut stmt = conn.prepare("SELECT v,tx,valid_from,valid_to,op,retracted_tx FROM facts WHERE g=0 AND e=?1 AND a=?2 ORDER BY rowid LIMIT 4097")?;
    let rows = stmt
        .query_map(params![e, a], |r| {
            Ok((
                r.get::<_, Vec<u8>>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, i32>(4)?,
                r.get::<_, Option<i64>>(5)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    if rows.len() > 4096 {
        return Err("history capture exceeds physical bound".into());
    }
    let mut history = Vec::new();
    let mut physical = Vec::new();
    for (bytes, tx, from, to, op, retracted_tx) in rows {
        let value = string(Value::from_bytes(&bytes)?)?;
        physical.push(
            json!({"value":value,"from":from,"to":to,"tx":tx,"op":op,"retracted_tx":retracted_tx}),
        );
        if op == 1 {
            history.push(json!({"value":value,"from":from,"to":to}));
        }
    }
    let mut pins = BTreeMap::from([
        ("base".to_string(), base_tx),
        ("before".to_string(), before),
        ("latest".to_string(), after),
    ]);
    let mut transactions = Vec::new();
    let mut native_transactions = Vec::new();
    if after < before || after - before > 16 {
        return Err("native transaction capture bound".into());
    }
    for id in (before + 1)..=after {
        let tx = store
            .get_transaction(id)?
            .ok_or("persisted transaction missing")?;
        let source = tx
            .source
            .as_deref()
            .ok_or("native receipt source missing")?;
        let role = receipt_role(source)?;
        if pins.insert(role.into(), id).is_some() {
            return Err("ambiguous native receipt role".into());
        }
        let mut asserted = Vec::new();
        let mut closed = Vec::new();
        for row in &physical {
            if row["op"] == 1 && row["tx"] == id {
                asserted.push(json!({"value":row["value"],"from":row["from"],"to":row["to"]}));
            }
            if row["op"] == 1 && row["retracted_tx"] == id {
                closed.push(json!({"value":row["value"],"at":row["to"]}));
            }
        }
        transactions.push(json!({"role":role,"close":closed,"assert":asserted}));
        native_transactions.push(json!({"id":id,"source":source,"role":role}));
    }
    let mut as_of = Vec::new();
    for probe in probes {
        let id = *pins
            .get(&probe.tx)
            .ok_or("requested receipt role unavailable; no guessed pin")?;
        let values = query(store, Some(id), Some(probe.valid_at.clone()))?;
        as_of.push(json!({"tx":probe.tx,"valid_at":probe.valid_at,"values":values}));
    }
    Ok(json!({"current":query(store,None,None)?,"history":history,
        "transactions":transactions,"as_of":as_of,"physical_history":physical,
        "native_transactions":native_transactions,"pins":pins}))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn receipt_binding_rejects_other_fork_or_unrecognized_step() {
        let source = format!(
            "fork-promote:{FORK};decisions=sha256:{};reviewer=oracle-input",
            "a".repeat(64)
        );
        assert_eq!(receipt_role(&source).unwrap(), "receive");
        assert_eq!(
            receipt_role(&format!("{source};step=decide")).unwrap(),
            "decide"
        );
        assert!(receipt_role(&format!("{source};step=other")).is_err());
        assert!(receipt_role("fork-promote:other").is_err());
    }
}
