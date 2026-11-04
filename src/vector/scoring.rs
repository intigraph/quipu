//! Score borrowed SQLite vector bytes without allocating two buffers per row.

/// The query norm is invariant across a scan. Decode little-endian coordinates
/// into the same f64 accumulators, in the same order as `cosine_similarity`.
/// No alignment, host-endianness or finite-value assumptions are added here.
pub(super) fn cosine_bytes(query: &[f32], bytes: &[u8], query_norm: f64) -> f64 {
    if query.is_empty() || bytes.len() / 4 != query.len() {
        return 0.0;
    }
    let mut dot = 0.0f64;
    let mut norm = 0.0f64;
    let (chunks, _) = bytes.as_chunks::<4>();
    for (x, chunk) in query.iter().zip(chunks) {
        let x = f64::from(*x);
        let y = f64::from(f32::from_le_bytes(*chunk));
        dot += x * y;
        norm += y * y;
    }
    let denominator = query_norm * norm.sqrt();
    if denominator == 0.0 {
        0.0
    } else {
        dot / denominator
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod benchmark;
