//! `xsd:dateTime` values compared as instants (aegis-dio), never by lexeme.
//!
//! `"2026-10-22T01:00:00+02:00"` and `"2026-10-21T23:00:00Z"` are the same
//! instant, and a lexical compare orders them the wrong way round. SPARQL 1.1
//! maps `=`, `<` and friends on `xsd:dateTime` to the `XPath` functions
//! `op:dateTime-equal` / `-less-than`, which compare on the timeline and give
//! an operand without a timezone the dynamic context's implicit timezone. This
//! store's implicit timezone is UTC, so every pair of valid dateTimes has a
//! determinate (total) order. Fractional seconds compare exactly, at any
//! number of digits.
use std::cmp::Ordering;

use crate::{Value, namespace};

const XSD_DATE_TIME_STAMP: &str = "http://www.w3.org/2001/XMLSchema#dateTimeStamp";
/// The widest timezone XSD allows, in seconds.
const MAX_TZ_SECS: i64 = 14 * 3_600;
/// The implicit timezone given to an operand that carries none (UTC).
const IMPLICIT_TZ_SECS: i64 = 0;

/// A parsed dateTime: local seconds since the epoch, the fractional-second
/// digits with trailing zeros removed, and the timezone offset in seconds when
/// the value carries one.
#[derive(Debug, Clone, PartialEq, Eq)]
struct DateTime {
    secs: i64,
    fraction: String,
    tz: Option<i64>,
}

impl DateTime {
    /// Seconds on the UTC timeline, taking the implicit timezone when absent.
    fn utc_secs(&self) -> i64 {
        self.secs - self.tz.unwrap_or(IMPLICIT_TZ_SECS)
    }
}

/// Order two fractional-second digit strings as the fractions they spell:
/// right-padding the shorter with zeros makes a lexical compare numeric.
fn cmp_fraction(a: &str, b: &str) -> Ordering {
    let width = a.len().max(b.len());
    format!("{a:0<width$}").cmp(&format!("{b:0<width$}"))
}

fn is_date_time(value: &Value) -> bool {
    matches!(value.datatype(), Some(d) if d == namespace::XSD_DATE_TIME || d == XSD_DATE_TIME_STAMP)
}

/// Parse the `xsd:dateTime` lexical space used here: a four-digit year,
/// optional fractional seconds, `24:00:00` as the next day's midnight, and an
/// optional `Z` / `+-HH:MM` timezone. `None` for anything else.
fn parse(lexical: &str) -> Option<DateTime> {
    let b = lexical.as_bytes();
    let num = |from: usize, to: usize| -> Option<i64> {
        let part = lexical.get(from..to)?;
        part.bytes()
            .all(|c| c.is_ascii_digit())
            .then(|| part.parse().ok())?
    };
    if b.len() < 19
        || b[4] != b'-'
        || b[7] != b'-'
        || b[10] != b'T'
        || b[13] != b':'
        || b[16] != b':'
    {
        return None;
    }
    let (y, mo, d) = (num(0, 4)?, num(5, 7)?, num(8, 10)?);
    let (hh, mi, ss) = (num(11, 13)?, num(14, 16)?, num(17, 19)?);
    let last_day = match mo {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 => 29,
        2 => 28,
        _ => return None,
    };
    if !(1..=last_day).contains(&d) || mi > 59 || ss > 59 {
        return None;
    }
    let mut i = 19;
    let mut fraction = String::new();
    if b.get(i) == Some(&b'.') {
        i += 1;
        let start = i;
        while b.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
        }
        if i == start {
            return None;
        }
        fraction = lexical[start..i].trim_end_matches('0').to_string();
    }
    // 24:00:00 is the first instant of the next day; any other 24:xx, including
    // one with a non-zero fraction at any precision, is invalid.
    let midnight_next = hh == 24 && mi == 0 && ss == 0 && fraction.is_empty();
    if hh > 23 && !midnight_next {
        return None;
    }
    let tz = match b.get(i) {
        None => None,
        Some(b'Z') if i + 1 == b.len() => Some(0),
        Some(&sign @ (b'+' | b'-')) if i + 6 == b.len() && b[i + 3] == b':' => {
            let (oh, om) = (num(i + 1, i + 3)?, num(i + 4, i + 6)?);
            let magnitude = oh * 3_600 + om * 60;
            if om > 59 || magnitude > MAX_TZ_SECS {
                return None;
            }
            Some(if sign == b'+' { magnitude } else { -magnitude })
        }
        _ => return None,
    };
    let secs = crate::time::days_from_civil(y, mo, d) * 86_400 + hh * 3_600 + mi * 60 + ss;
    Some(DateTime { secs, fraction, tz })
}

fn order(a: &DateTime, b: &DateTime) -> Ordering {
    a.utc_secs()
        .cmp(&b.utc_secs())
        .then_with(|| cmp_fraction(&a.fraction, &b.fraction))
}

/// The instant ordering of two `xsd:dateTime` values, or `None` when either
/// is not a valid dateTime (callers then keep their existing comparison). Use
/// [`both_date_times`] to decide whether this comparison applies.
pub(crate) fn compare(a: &Value, b: &Value) -> Option<Ordering> {
    Some(order(&parse(a.as_lexical()?)?, &parse(b.as_lexical()?)?))
}

/// True when both values are `xsd:dateTime`s whose lexical forms parse, so
/// [`compare`] is the comparison that applies to them.
pub(crate) fn both_date_times(a: &Value, b: &Value) -> bool {
    let parses = |v: &Value| is_date_time(v) && v.as_lexical().and_then(parse).is_some();
    parses(a) && parses(b)
}

#[cfg(test)]
#[path = "datetime_value_tests.rs"]
mod tests;
