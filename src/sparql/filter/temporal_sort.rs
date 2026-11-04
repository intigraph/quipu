//! ORDER BY / MIN / MAX order for temporal literals (aegis-neg).
use super::{Temporal, temporal};
use crate::types::Value;

/// The ORDER BY (and MIN / MAX) order where a temporal literal is involved;
/// `None` when neither side is temporal, so the caller keeps its order.
///
/// ORDER BY must be a total order, while XSD temporals are only partially
/// ordered, so this extends `temporal_order` deterministically (SPARQL 1.1
/// section 15.1 leaves the order among literals of different types, and of
/// incomparable values, to the implementation):
/// - A temporal sorts after every non-temporal value (unbound, IRIs and other
///   literals keep their existing relative order, ahead of it).
/// - Kinds sort `dateTime` (and `dateTimeStamp`), then `date`, then `time`.
/// - Within a kind, valid values precede ill-typed lexemes, which compare by
///   lexical form.
/// - Valid values compare by value with offsets normalised: the key is the
///   instant, reading an untimezoned value as UTC (the implicit timezone).
///   That agrees with every determinate XSD comparison; an indeterminate
///   pair (one side untimezoned, within 14h) takes the UTC reading, and at an
///   equal key the untimezoned value sorts first. A lexical fallback for
///   indeterminate pairs is not transitive (20:00-10:00 < 21:00 < 23:00Z
///   lexically, but 20:00-10:00 is later than 23:00Z).
/// - Equal instants (`10:00Z`, `12:00+02:00`) compare `Equal`, so the stable
///   sort keeps their input order.
pub(in crate::sparql) fn temporal_sort_order(a: &Value, b: &Value) -> Option<std::cmp::Ordering> {
    use std::cmp::Ordering;
    let (ka, kb) = (temporal(a.datatype()), temporal(b.datatype()));
    let rank = |kind: Temporal| match kind {
        Temporal::DateTime => 0,
        Temporal::Date => 1,
        Temporal::Time => 2,
    };
    let (ka, kb) = match (ka, kb) {
        (None, None) => return None,
        (Some(_), None) => return Some(Ordering::Greater),
        (None, Some(_)) => return Some(Ordering::Less),
        (Some(ka), Some(kb)) if ka != kb => return Some(rank(ka).cmp(&rank(kb))),
        (Some(ka), Some(kb)) => (ka, kb),
    };
    debug_assert!(ka == kb);
    let (x, y) = (
        a.as_lexical().unwrap_or_default().trim(),
        b.as_lexical().unwrap_or_default().trim(),
    );
    fn by_value<T: std::str::FromStr + PartialOrd + Copy>(
        x: &str,
        y: &str,
        has_tz: impl Fn(T) -> bool,
        as_utc: impl Fn(T) -> Option<T>,
    ) -> Option<Ordering> {
        match (x.parse::<T>(), y.parse::<T>()) {
            (Ok(x), Ok(y)) => {
                let (tx, ty) = (has_tz(x), has_tz(y));
                if tx == ty {
                    // Same timezone-ness is totally ordered by value.
                    return Some(x.partial_cmp(&y).unwrap_or(Ordering::Equal));
                }
                let (ux, uy) = (as_utc(x)?, as_utc(y)?);
                Some(
                    ux.partial_cmp(&uy)
                        .unwrap_or(Ordering::Equal)
                        .then(tx.cmp(&ty)),
                )
            }
            (Ok(_), Err(_)) => Some(Ordering::Less),
            (Err(_), Ok(_)) => Some(Ordering::Greater),
            (Err(_), Err(_)) => None,
        }
    }
    let utc = Some(oxsdatatypes::TimezoneOffset::UTC);
    let order = match ka {
        Temporal::DateTime => by_value::<oxsdatatypes::DateTime>(
            x,
            y,
            |v| v.timezone_offset().is_some(),
            |v| v.adjust(utc),
        ),
        Temporal::Date => by_value::<oxsdatatypes::Date>(
            x,
            y,
            |v| v.timezone_offset().is_some(),
            |v| v.adjust(utc),
        ),
        Temporal::Time => by_value::<oxsdatatypes::Time>(
            x,
            y,
            |v| v.timezone_offset().is_some(),
            |v| v.adjust(utc),
        ),
    };
    // Both ill-typed (or an overflow adjusting to UTC): lexical form.
    Some(order.unwrap_or_else(|| x.cmp(y)))
}
