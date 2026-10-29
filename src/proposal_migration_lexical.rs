//! Lexical-form checks for the XSD datatypes a migration may cast to.

use std::sync::LazyLock;

use crate::namespace;

static XSD_LEXICAL: LazyLock<Vec<(&'static str, regex::Regex)>> = LazyLock::new(|| {
    let integer = r"^[+-]?[0-9]+$";
    let decimal = r"^[+-]?([0-9]+(\.[0-9]*)?|\.[0-9]+)$";
    let double = r"^([+-]?([0-9]+(\.[0-9]*)?|\.[0-9]+)([eE][+-]?[0-9]+)?|[+-]?INF|NaN)$";
    let tz = r"(Z|[+-][0-9]{2}:[0-9]{2})?";
    let date = format!(r"^-?[0-9]{{4,}}-(0[1-9]|1[0-2])-(0[1-9]|[12][0-9]|3[01]){tz}$");
    let date_time = format!(
        r"^-?[0-9]{{4,}}-(0[1-9]|1[0-2])-(0[1-9]|[12][0-9]|3[01])T([01][0-9]|2[0-4]):[0-5][0-9]:[0-5][0-9](\.[0-9]+)?{tz}$"
    );
    let xsd = |local: &str| -> &'static str {
        Box::leak(format!("{}{local}", namespace::XSD).into_boxed_str())
    };
    let mut table = Vec::new();
    for local in [
        "integer",
        "int",
        "long",
        "short",
        "byte",
        "nonNegativeInteger",
        "positiveInteger",
        "nonPositiveInteger",
        "negativeInteger",
        "unsignedInt",
        "unsignedLong",
        "unsignedShort",
        "unsignedByte",
    ] {
        table.push((
            xsd(local),
            regex::Regex::new(integer).expect("static regex"),
        ));
    }
    table.push((
        xsd("decimal"),
        regex::Regex::new(decimal).expect("static regex"),
    ));
    table.push((
        xsd("double"),
        regex::Regex::new(double).expect("static regex"),
    ));
    table.push((
        xsd("float"),
        regex::Regex::new(double).expect("static regex"),
    ));
    table.push((
        xsd("boolean"),
        regex::Regex::new("^(true|false|1|0)$").expect("static regex"),
    ));
    table.push((xsd("date"), regex::Regex::new(&date).expect("static regex")));
    table.push((
        xsd("dateTime"),
        regex::Regex::new(&date_time).expect("static regex"),
    ));
    table
});

/// Whether `lexical` is a valid form of `datatype`; datatypes not listed here
/// are not checked.
pub(super) fn well_typed(lexical: &str, datatype: &str) -> bool {
    let local = datatype.strip_prefix(namespace::XSD).unwrap_or("");
    // The XML Schema parsers check calendar validity and timezone bounds,
    // which a lexical date regex alone cannot establish.
    match local {
        "date" => return lexical.parse::<oxsdatatypes::Date>().is_ok(),
        "dateTime" => return lexical.parse::<oxsdatatypes::DateTime>().is_ok(),
        _ => {}
    }
    let lexical_ok = XSD_LEXICAL
        .iter()
        .find(|(dt, _)| *dt == datatype)
        .is_none_or(|(_, pattern)| pattern.is_match(lexical));
    if !lexical_ok {
        return false;
    }
    let digits = lexical
        .trim_start_matches(['+', '-'])
        .trim_start_matches('0');
    let negative = lexical.starts_with('-') && !digits.is_empty();
    let zero = digits.is_empty();
    match local {
        "byte" => lexical.parse::<i8>().is_ok(),
        "short" => lexical.parse::<i16>().is_ok(),
        "int" => lexical.parse::<i32>().is_ok(),
        "long" => lexical.parse::<i64>().is_ok(),
        "unsignedByte" => !negative && unsigned(digits, u64::from(u8::MAX)),
        "unsignedShort" => !negative && unsigned(digits, u64::from(u16::MAX)),
        "unsignedInt" => !negative && unsigned(digits, u64::from(u32::MAX)),
        "unsignedLong" => !negative && unsigned(digits, u64::MAX),
        "nonNegativeInteger" => !negative,
        "positiveInteger" => !negative && !zero,
        "nonPositiveInteger" => negative || zero,
        "negativeInteger" => negative,
        _ => true,
    }
}

fn unsigned(digits: &str, max: u64) -> bool {
    digits.is_empty() || digits.parse::<u64>().is_ok_and(|value| value <= max)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restricted_integer_value_boundaries_and_calendar_dates() {
        for (kind, valid, invalid) in [
            ("byte", "-128", "-129"),
            ("byte", "127", "128"),
            ("short", "32767", "32768"),
            ("int", "2147483647", "2147483648"),
            ("long", "9223372036854775807", "9223372036854775808"),
            ("unsignedByte", "255", "256"),
            ("unsignedShort", "65535", "65536"),
            ("unsignedInt", "4294967295", "4294967296"),
            (
                "unsignedLong",
                "18446744073709551615",
                "18446744073709551616",
            ),
            ("positiveInteger", "1", "0"),
            ("negativeInteger", "-1", "-0"),
            ("nonNegativeInteger", "-0", "-1"),
            ("nonPositiveInteger", "+0", "1"),
            ("date", "2024-02-29Z", "2026-02-29Z"),
            ("dateTime", "2024-02-29T00:00:00Z", "2026-02-31T00:00:00Z"),
        ] {
            let datatype = format!("{}{kind}", namespace::XSD);
            assert!(well_typed(valid, &datatype), "{kind}: {valid}");
            assert!(!well_typed(invalid, &datatype), "{kind}: {invalid}");
        }
        for (kind, value) in [
            ("integer", "999999999999999999999999999999"),
            ("positiveInteger", "999999999999999999999999999999"),
            ("unsignedLong", "-000000"),
            ("unsignedByte", "+000255"),
        ] {
            assert!(well_typed(value, &format!("{}{kind}", namespace::XSD)));
        }
    }
}
