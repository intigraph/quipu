//! Exact strsim 0.11 Jaro-Winkler scores, with reusable character/flag buffers.
//! A conservative upper bound may reject a pair early; retained scores keep
//! strsim's integer transposition rounding and floating-point operation order.
pub(super) struct Matcher {
    name: Vec<char>,
    other: Vec<char>,
    flags: Vec<bool>,
}

#[cfg(test)]
#[path = "jaro_tests.rs"]
mod tests;

impl Matcher {
    pub(super) fn new(name: &str) -> Self {
        Self {
            name: name.chars().collect(),
            other: Vec::new(),
            flags: Vec::new(),
        }
    }

    #[allow(clippy::cast_precision_loss)]
    pub(super) fn score(&mut self, other: &str, threshold: f64) -> Option<f64> {
        self.other.clear();
        self.other.extend(other.chars());
        let (a, b) = (&self.name, &self.other);
        if a.is_empty() || b.is_empty() {
            return Some(if a.is_empty() && b.is_empty() {
                1.0
            } else {
                0.0
            });
        }
        self.flags.resize(a.len() + b.len(), false);
        self.flags.fill(false);
        let (a_flags, b_flags) = self.flags.split_at_mut(a.len());
        let window = (a.len().max(b.len()) / 2).saturating_sub(1);
        let prefix = a.iter().zip(b).take(4).take_while(|(x, y)| x == y).count() as f64;
        let mut matches = 0usize;
        for (i, c) in a.iter().enumerate() {
            let lower = i.saturating_sub(window);
            let upper = b.len().min(i + window + 1);
            for j in lower..upper {
                if *c == b[j] && !b_flags[j] {
                    a_flags[i] = true;
                    b_flags[j] = true;
                    matches += 1;
                    break;
                }
            }
            // At most every remaining character can add one match. Ignore
            // transpositions and allow the known prefix boost even below
            // Jaro's 0.7 boost threshold: both choices only raise the bound.
            // Check in small batches to avoid divisions on every character.
            if i % 8 == 7 || i + 1 == a.len() {
                let m = (matches + a.len() - i - 1).min(b.len()) as f64;
                let upper_jaro = if m == 0.0 {
                    0.0
                } else {
                    (m / a.len() as f64 + m / b.len() as f64 + 1.0) / 3.0
                };
                let bound =
                    upper_jaro + 0.1 * prefix * (1.0 - upper_jaro) + super::index::JW_BOUND_MARGIN;
                if bound < threshold {
                    return None;
                }
            }
        }
        if matches == 0 {
            return Some(0.0);
        }
        let mut b_matches = b_flags.iter().zip(b).filter(|(flag, _)| **flag);
        let mut transpositions = 0usize;
        for (flag, c) in a_flags.iter().zip(a) {
            if *flag
                && let Some((_, other)) = b_matches.next()
                && c != other
            {
                transpositions += 1;
            }
        }
        transpositions /= 2;
        let sim = ((matches as f64 / a.len() as f64)
            + (matches as f64 / b.len() as f64)
            + ((matches - transpositions) as f64 / matches as f64))
            / 3.0;
        Some(if sim > 0.7 {
            sim + 0.1 * prefix * (1.0 - sim)
        } else {
            sim
        })
    }
}
