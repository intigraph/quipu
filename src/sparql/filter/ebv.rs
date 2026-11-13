//! Effective boolean values for bound RDF terms.
use crate::{namespace, types::Value};

/// SPARQL effective boolean value for a bound value used directly as a FILTER.
pub(in crate::sparql) fn effective_boolean_value(v: &Value) -> Option<bool> {
    match v {
        Value::Bool(b) => Some(*b),
        Value::Str(s) => Some(!s.is_empty()),
        Value::Int(i) => Some(*i != 0),
        Value::Float(f) => Some(*f != 0.0 && !f.is_nan()),
        // SPARQL EBV: numeric literals test against zero, plain/lang strings
        // against emptiness. Other datatypes have no EBV.
        Value::Typed { lexical, datatype } if namespace::is_numeric_datatype(datatype) => Some(
            lexical
                .parse::<f64>()
                .is_ok_and(|f| f != 0.0 && !f.is_nan()),
        ),
        Value::Typed { lexical, datatype } if datatype == namespace::XSD_BOOLEAN => {
            Some(matches!(lexical.as_str(), "true" | "1"))
        }
        Value::Lang { lexical, .. } => Some(!lexical.is_empty()),
        Value::Typed { .. } | Value::Ref(_) | Value::Bytes(_) | Value::Triple(_) => None,
    }
}
