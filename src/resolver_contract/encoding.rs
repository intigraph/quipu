//! Domain-separated, typed, length-framed encoding, independent of JSON layout.

use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::ContentDigest;
use crate::{Error, Result};

pub(super) fn digest(bytes: &[u8]) -> ContentDigest {
    ContentDigest(Sha256::digest(bytes).into())
}

fn length(out: &mut Vec<u8>, n: usize) -> Result<()> {
    let n = u64::try_from(n).map_err(|_| Error::InvalidValue("resolver length overflow".into()))?;
    out.extend_from_slice(&n.to_be_bytes());
    Ok(())
}

fn bytes(out: &mut Vec<u8>, value: &[u8]) -> Result<()> {
    length(out, value.len())?;
    out.extend_from_slice(value);
    Ok(())
}

pub(super) fn encode(domain: &[u8], value: &impl Serialize) -> Result<Vec<u8>> {
    let value =
        serde_json::to_value(value).map_err(|error| Error::Serialization(error.to_string()))?;
    let mut out = domain.to_vec();
    frame(&mut out, &value)?;
    Ok(out)
}

fn frame(out: &mut Vec<u8>, value: &Value) -> Result<()> {
    match value {
        Value::Null => out.push(b'n'),
        Value::Bool(v) => out.extend_from_slice(if *v { b"b1" } else { b"b0" }),
        Value::Number(v) => {
            out.push(b'u');
            out.extend_from_slice(
                &v.as_u64()
                    .ok_or_else(|| {
                        Error::InvalidValue("resolver requires unsigned integers".into())
                    })?
                    .to_be_bytes(),
            );
        }
        Value::String(v) => {
            out.push(b's');
            bytes(out, v.as_bytes())?;
        }
        Value::Array(values) => {
            // JSON has no byte scalar. A nonempty unsigned-octet array has one
            // canonical compact form, including digests and source contents.
            if !values.is_empty() && values.iter().all(|v| v.as_u64().is_some_and(|v| v <= 255)) {
                out.push(b'v');
                length(out, values.len())?;
                for value in values {
                    out.push(
                        u8::try_from(value.as_u64().expect("checked unsigned octet"))
                            .expect("checked octet range"),
                    );
                }
                return Ok(());
            }
            out.push(b'a');
            length(out, values.len())?;
            for v in values {
                frame(out, v)?;
            }
        }
        Value::Object(values) => {
            out.push(b'o');
            length(out, values.len())?;
            let mut keys: Vec<_> = values.keys().collect();
            keys.sort();
            for key in keys {
                bytes(out, key.as_bytes())?;
                frame(out, &values[key])?;
            }
        }
    }
    Ok(())
}
