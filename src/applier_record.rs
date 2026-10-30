//! Feed records decoded into the datums that reproduce them exactly, or
//! refused when they cannot be.

use serde_json::Value as Json;

use super::digest;
use crate::error::Result;
use crate::store::{Datum, Store};
use crate::types::{Op, Value};

/// One feed record as the datum that reproduces it, or why it cannot be
/// reproduced exactly. The outer `Result` is a store failure.
///
/// # Errors
/// [`Error::Sqlite`] if interning a term fails.
pub fn record_to_datum(
    store: &Store,
    record: &Json,
    tx_timestamp: &str,
) -> Result<std::result::Result<Datum, String>> {
    let (Some(entity), Some(attribute), Some(op)) = (
        record["entity"].as_str(),
        record["attribute"].as_str(),
        record["op"].as_str(),
    ) else {
        return Ok(Err("record lacks entity, attribute or op".into()));
    };
    let retract = match digest::op_is_retract(op) {
        Ok(r) => r,
        Err(reason) => return Ok(Err(reason)),
    };
    let close = match retracted_valid_at(record, retract) {
        Ok(close) => close,
        Err(reason) => return Ok(Err(reason)),
    };
    if retract && close.is_none() {
        return Ok(Err(
            "legacy retraction lacks retracted_valid_at; upgrade the source instead of guessing its close".into(),
        ));
    }
    let value_field = if retract { "old_value" } else { "value" };
    let value = match value_of(store, &record[value_field])? {
        Ok(v) => v,
        Err(reason) => return Ok(Err(reason)),
    };
    let valid_from = record["valid_from"]
        .as_str()
        .unwrap_or(tx_timestamp)
        .to_string();
    Ok(Ok(Datum {
        entity: store.intern(entity)?,
        attribute: store.intern(attribute)?,
        value,
        valid_from,
        // The retraction's audit interval is not the assertion's close.
        valid_to: if retract {
            close.map(String::from)
        } else {
            record["valid_to"].as_str().map(String::from)
        },
        op: if retract { Op::Retract } else { Op::Assert },
    }))
}

/// Shared by decoding and digesting: malformed new metadata never falls back
/// to a guessed close. Absence denotes legacy digest encoding only; decoding
/// a new retraction without a close is refused above.
pub(super) fn retracted_valid_at(
    record: &Json,
    retract: bool,
) -> std::result::Result<Option<&str>, String> {
    let Some(value) = record.get("retracted_valid_at") else {
        return Ok(None);
    };
    let Some(close) = value.as_str().filter(|_| retract) else {
        return Err("retracted_valid_at requires a retraction and a timestamp string".into());
    };
    let valid = crate::time::normalize_rfc3339_utc(close).is_some()
        || (close.len() == 10
            && crate::time::normalize_rfc3339_utc(&format!("{close}T00:00:00Z")).is_some());
    if !valid {
        return Err("retracted_valid_at is not an ISO date or RFC 3339 timestamp".into());
    }
    Ok(Some(close))
}

fn value_of(store: &Store, json: &Json) -> Result<std::result::Result<Value, String>> {
    use base64::Engine as _;
    Ok(Ok(match json {
        Json::String(s) => Value::Str(s.clone()),
        Json::Bool(b) => Value::Bool(*b),
        Json::Number(n) => match n.as_i64() {
            Some(i) => Value::Int(i),
            None => match n.as_f64() {
                Some(f) => Value::Float(f),
                None => return Ok(Err(format!("unrepresentable number {n}"))),
            },
        },
        Json::Object(o) if o.get("ref").and_then(Json::as_str).is_some() => {
            Value::Ref(store.intern(o["ref"].as_str().unwrap_or_default())?)
        }
        Json::Object(o) if o.contains_key("lang") => Value::Lang {
            lexical: o["value"].as_str().unwrap_or_default().into(),
            lang: o["lang"].as_str().unwrap_or_default().into(),
        },
        Json::Object(o) if o.contains_key("datatype") => Value::Typed {
            lexical: o["value"].as_str().unwrap_or_default().into(),
            datatype: o["datatype"].as_str().unwrap_or_default().into(),
        },
        Json::Object(o) if o.get("base64").and_then(Json::as_str).is_some() => {
            match base64::engine::general_purpose::STANDARD
                .decode(o["base64"].as_str().unwrap_or_default())
            {
                Ok(bytes) => Value::Bytes(bytes),
                Err(_) => return Ok(Err("byte value is not valid base64".into())),
            }
        }
        // A source from before byte bodies were carried sends only the length.
        // Writing empty bytes would be a silent loss: refuse instead.
        Json::Object(o) if o.contains_key("bytes_len") => {
            return Ok(Err(
                "byte value sent as its length only; the source predates lossless bytes".into(),
            ));
        }
        other => return Ok(Err(format!("unreplayable value {other}"))),
    }))
}
