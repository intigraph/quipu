//! `xsd:dateTime` values compared as instants (aegis-dio), never by lexeme.
//!
//! `"2026-10-22T01:00:00+02:00"` and `"2026-10-21T23:00:00Z"` are the same
//! instant, and a lexical compare orders them the wrong way round. SPARQL 1.1
//! defines `=`, `<` and friends on `xsd:dateTime` values through the `XPath` functions
//! `op:dateTime-equal` / `-less-than`: values are compared on the timeline.
//! A value without a timezone is compared directly with another untimezoned
//! value; against a timezoned one it may sit anywhere within +-14:00, so the
//! answer is known only outside that window and is otherwise indeterminate
//! (an error, which a FILTER treats as false).
use std::cmp::Ordering;

use crate::{Value, namespace};

const XSD_DATE_TIME_STAMP: &str = "http://www.w3.org/2001/XMLSchema#dateTimeStamp";
/// The widest timezone XSD allows, in seconds.
const MAX_TZ_SECS: i64 = 14 * 3_600;

/// A parsed dateTime: local seconds since the epoch, nanoseconds, and the
/// timezone offset in seconds when the value carries one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DateTime {
    secs: i64,
    nanos: u32,
    tz: Option<i64>,
}

impl DateTime {
    /// Seconds on the UTC timeline (local time minus the offset).
    fn utc(self, tz: i64) -> (i64, u32) {
        (self.secs - tz, self.nanos)
    }
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
    let mut nanos = 0u32;
    if b.get(i) == Some(&b'.') {
        i += 1;
        let start = i;
        while b.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
        }
        if i == start {
            return None;
        }
        // Digits past nanoseconds cannot change an ordering this store needs.
        let digits = &lexical[start..i.min(start + 9)];
        nanos = format!("{digits:0<9}").parse().ok()?;
    }
    // 24:00:00 is the first instant of the next day; any other 24:xx is invalid.
    let midnight_next = hh == 24 && mi == 0 && ss == 0 && nanos == 0;
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
    Some(DateTime { secs, nanos, tz })
}

fn order(a: DateTime, b: DateTime) -> Option<Ordering> {
    match (a.tz, b.tz) {
        (Some(ta), Some(tb)) => Some(a.utc(ta).cmp(&b.utc(tb))),
        (None, None) => Some((a.secs, a.nanos).cmp(&(b.secs, b.nanos))),
        // `b` lies somewhere in [b - 14h, b + 14h] on the UTC timeline.
        (Some(ta), None) => {
            let a = a.utc(ta);
            if a < (b.secs - MAX_TZ_SECS, b.nanos) {
                Some(Ordering::Less)
            } else if a > (b.secs + MAX_TZ_SECS, b.nanos) {
                Some(Ordering::Greater)
            } else {
                None
            }
        }
        (None, Some(_)) => order(b, a).map(Ordering::reverse),
    }
}

/// The instant ordering of two `xsd:dateTime` values: `Some` when both are
/// valid dateTimes with a determinate order, `None` when either is not a
/// dateTime (callers keep their existing comparison) or the order is
/// indeterminate. Use [`both_date_times`] to tell those apart.
pub(crate) fn compare(a: &Value, b: &Value) -> Option<Ordering> {
    order(parse(a.as_lexical()?)?, parse(b.as_lexical()?)?)
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
