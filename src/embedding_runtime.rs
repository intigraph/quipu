//! Shared provider setup and immutable model identity for native loaders.
use serde::{Deserialize, Serialize};

use crate::{Error, Result};

/// Exact ONNX and tokenizer bytes, not a model-family or download URL claim.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmbeddingRecipe {
    /// ONNX filename only; never a host path.
    pub model: String,
    /// Lowercase SHA-256 of the actual ONNX bytes.
    pub model_sha256: String,
    /// Lowercase SHA-256 of the actual tokenizer bytes.
    pub tokenizer_sha256: String,
    /// Validated model output width.
    pub dimension: usize,
}

impl EmbeddingRecipe {
    /// Reject malformed or path-bearing recipe metadata.
    pub fn validate(&self) -> Result<()> {
        let digest = |s: &str| {
            s.len() == 64
                && s.bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        };
        if self.model.is_empty()
            || self
                .model
                .chars()
                .any(|c| c.is_control() || c == '/' || c == '\\')
            || !digest(&self.model_sha256)
            || !digest(&self.tokenizer_sha256)
            || !(1..=16_384).contains(&self.dimension)
        {
            return Err(Error::InvalidValue(
                "invalid pendant embedding recipe".into(),
            ));
        }
        Ok(())
    }

    /// Identify the same immutable byte buffers passed to the runtime.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn from_bytes(
        model_name: &str,
        model: &[u8],
        tokenizer: &[u8],
        dimension: usize,
    ) -> Result<Self> {
        let hash = |bytes| hex::encode(ring::digest::digest(&ring::digest::SHA256, bytes).as_ref());
        let recipe = Self {
            model: model_name.into(),
            model_sha256: hash(model),
            tokenizer_sha256: hash(tokenizer),
            dimension,
        };
        recipe.validate()?;
        Ok(recipe)
    }
}

/// What [`configure_provider`] set up. `Display` is the operator-facing line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderStatus {
    /// A model and tokenizer loaded.
    Ready {
        /// Embedding width the provider produces.
        dimension: usize,
        /// Whether writes are embedded automatically.
        auto_embed: bool,
    },
    /// `auto_embed = false` and no model: the operator turned it off.
    Disabled,
    /// No model configured. The default for a fresh local store, so a CLI need
    /// not announce it on every command: semantic calls still refuse with their
    /// own "no embedding provider configured" error. A server logs it once.
    Unconfigured,
}

impl std::fmt::Display for ProviderStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Ready { dimension, auto_embed } => {
                write!(f, "embedding provider ready (dim={dimension}, auto_embed={auto_embed})")
            }
            Self::Disabled => f.write_str("automatic embedding disabled by explicit configuration"),
            Self::Unconfigured => f.write_str("automatic embedding unavailable: no model and tokenizer configured; graph loading is not semantic-index readiness"),
        }
    }
}

/// Applies explicit opt-out as well as the default. No model is downloaded.
/// A missing provider leaves graph operations usable, but never claims an index.
#[cfg(not(target_arch = "wasm32"))]
pub fn configure_provider(
    store: &mut crate::Store,
    config: &crate::EmbeddingConfig,
) -> Result<ProviderStatus> {
    store.embedding_config_mut().clone_from(config);
    match (&config.model_path, &config.tokenizer_path) {
        (Some(model), Some(tokenizer)) => {
            #[cfg(feature = "onnx")]
            {
                use std::sync::Arc;
                let provider = crate::OnnxEmbeddingProvider::load(
                    model,
                    tokenizer,
                    config.dimension,
                    config.max_sequence_length,
                )?;
                store.set_embedding_provider(Arc::new(provider));
                Ok(ProviderStatus::Ready {
                    dimension: config.dimension,
                    auto_embed: config.auto_embed,
                })
            }
            #[cfg(not(feature = "onnx"))]
            {
                let _ = (model, tokenizer);
                Err(Error::InvalidValue("configured embedding model requires the onnx build feature; no index was created".into()))
            }
        }
        (None, None) if !config.auto_embed => Ok(ProviderStatus::Disabled),
        (None, None) => Ok(ProviderStatus::Unconfigured),
        _ => Err(Error::InvalidValue(
            "embedding requires BOTH model_path and tokenizer_path; no index was created".into(),
        )),
    }
}
