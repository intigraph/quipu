//! Embedding configuration and explicit operator intent.

use std::path::PathBuf;

/// Embedding configuration for auto-embedding on write.
#[derive(Debug, Clone)]
pub struct EmbeddingConfig {
    /// Whether to auto-embed entities after writes (default: true).
    pub auto_embed: bool,

    /// Whether `auto_embed` was explicitly supplied by the operator.
    pub auto_embed_explicit: bool,

    /// Number of entities to embed in each batch (default: 32).
    pub embed_batch_size: usize,

    /// Path to the ONNX model file (e.g. all-MiniLM-L6-v2/onnx/model.onnx).
    pub model_path: Option<PathBuf>,

    /// Path to the tokenizer.json file (same directory as model typically).
    pub tokenizer_path: Option<PathBuf>,

    /// Embedding dimension (default: 384 for all-MiniLM-L6-v2).
    pub dimension: usize,

    /// Maximum input tokens fed to the model; longer inputs are truncated
    /// (default: 256). Caps the tensor size so an oversized `episode_body`
    /// can't blow up the sequence length or degrade embeddings (hq-7v0).
    pub max_sequence_length: usize,

    /// Predicate IRIs whose values never enter an entity's embedding text
    /// (default: none). For values that change on every write but carry no
    /// meaning to search, such as a write-probe marker: each new value would
    /// otherwise make new text and so a new vector, while the identical-text
    /// skip keeps the vector when only an excluded predicate changed. The
    /// facts are still stored and queryable; only the embedding ignores them.
    pub exclude_predicates: Vec<String>,

    /// Days a superseded vector (one whose `valid_to` is set) is kept before
    /// the server deletes it (default: 30; `0` keeps every version forever).
    /// Current vectors are never deleted. The cost is recall for
    /// `search --valid-at`: a point in time older than this window finds
    /// only the entities whose vector was still current then or is current
    /// now, not the older text of entities that changed since. A row whose
    /// `valid_to` is not a parseable timestamp is kept.
    pub superseded_retention_days: u32,

    /// Most superseded vectors deleted in one hourly retention pass
    /// (default: 50 000). A store upgraded with a large backlog drains it
    /// over several passes instead of in one long delete.
    pub superseded_prune_max_per_pass: usize,
}

/// Default for [`EmbeddingConfig::superseded_retention_days`].
pub const DEFAULT_SUPERSEDED_RETENTION_DAYS: u32 = 30;

/// Default for [`EmbeddingConfig::superseded_prune_max_per_pass`].
pub const DEFAULT_SUPERSEDED_PRUNE_MAX_PER_PASS: usize = 50_000;

impl Default for EmbeddingConfig {
    fn default() -> Self {
        Self {
            auto_embed: true,
            auto_embed_explicit: false,
            embed_batch_size: 32,
            model_path: None,
            tokenizer_path: None,
            dimension: 384,
            max_sequence_length: 256,
            exclude_predicates: Vec::new(),
            superseded_retention_days: DEFAULT_SUPERSEDED_RETENTION_DAYS,
            superseded_prune_max_per_pass: DEFAULT_SUPERSEDED_PRUNE_MAX_PER_PASS,
        }
    }
}

impl<'de> serde::Deserialize<'de> for EmbeddingConfig {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        let value = serde_json::Value::deserialize(deserializer)?;
        let serde_json::Value::Object(mut fields) = value else {
            return Err(serde::de::Error::custom(
                "embedding configuration must be a table",
            ));
        };
        let mut config = Self {
            auto_embed_explicit: fields.contains_key("auto_embed"),
            ..Self::default()
        };
        macro_rules! read {
            ($($field:ident),* $(,)?) => { $(
                if let Some(value) = fields.remove(stringify!($field)) {
                    config.$field = serde_json::from_value(value).map_err(serde::de::Error::custom)?;
                }
            )* };
        }
        read!(
            auto_embed,
            embed_batch_size,
            model_path,
            tokenizer_path,
            dimension,
            max_sequence_length,
            exclude_predicates,
            superseded_retention_days,
            superseded_prune_max_per_pass
        );
        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicitness_is_derived_from_the_effective_setting_only() {
        let omitted: EmbeddingConfig = toml::from_str("").unwrap();
        assert!(omitted.auto_embed);
        assert!(!omitted.auto_embed_explicit);
        for enabled in [true, false] {
            let config: EmbeddingConfig =
                toml::from_str(&format!("auto_embed = {enabled}")).unwrap();
            assert_eq!(config.auto_embed, enabled);
            assert!(config.auto_embed_explicit);
        }
        let forged: EmbeddingConfig = toml::from_str("auto_embed_explicit = true").unwrap();
        assert!(!forged.auto_embed_explicit);
        assert!(toml::from_str::<EmbeddingConfig>("auto_embed = 'true'").is_err());
    }
}
