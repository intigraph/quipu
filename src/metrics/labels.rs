//! Normalization of declared request labels.

/// Normalise a caller identity into a bounded metric label (aegis-ma1hy).
///
/// Precedence: the explicit `X-Quipu-Client` header, then `User-Agent`, then
/// `unattributed`. Everything is lowercased, cut at the first `/` (so
/// `curl/8.5.0` and `yupana/0.4.1` collapse to `curl` and `yupana` rather than
/// minting a label per released version), restricted to `[a-z0-9._-]`, and
/// truncated to 32 bytes.
///
/// `unattributed` is a REAL answer, not a failure: it is the measure of how much
/// load still cannot be attributed, which is the number aegis-ma1hy exists to
/// drive down. Callers that send nothing must be visible as a bloc, never
/// silently dropped.
#[must_use]
pub fn normalize_client(explicit: Option<&str>, user_agent: Option<&str>) -> String {
    let raw = explicit
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .or_else(|| user_agent.map(str::trim).filter(|s| !s.is_empty()));
    let Some(raw) = raw else {
        return "unattributed".to_string();
    };
    let cleaned: String = raw
        // Cut at the first `/` OR any whitespace. Splitting on newlines matters
        // beyond tidiness: a raw newline in a label value would break the
        // exposition format outright, and this header is caller-controlled.
        // The char filter below would also strip it, but two independent
        // defences for a format-injection vector reachable by any caller is the
        // right number.
        .split(['/', ' ', '\t', '\n', '\r'])
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
        .take(32)
        .collect();
    if cleaned.is_empty() {
        "unattributed".to_string()
    } else {
        cleaned
    }
}

/// Normalise the optional work-item identity supplied in `X-Quipu-Task`.
///
/// Unlike [`normalize_client`], this has no fallback: a User-Agent identifies
/// software, never the bead whose work caused the request. Missing or invalid
/// values are therefore explicitly `unattributed` rather than silently joined
/// to an arbitrary task. The independent cardinality cap in [`request_key`]
/// folds excess task identities into `other`.
#[must_use]
pub fn normalize_task(explicit: Option<&str>) -> String {
    let Some(raw) = explicit.map(str::trim).filter(|s| !s.is_empty()) else {
        return "unattributed".to_string();
    };
    let cleaned: String = raw
        .to_ascii_lowercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
        .take(64)
        .collect();
    if cleaned.is_empty() {
        "unattributed".to_string()
    } else {
        cleaned
    }
}
