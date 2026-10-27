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
    XSD_LEXICAL
        .iter()
        .find(|(dt, _)| *dt == datatype)
        .is_none_or(|(_, pattern)| pattern.is_match(lexical))
}
