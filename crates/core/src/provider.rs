use async_trait::async_trait;
use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};

use crate::config::Config;
use crate::error::{AlexandriaError, Result};

/// Prompt wrapper for LLM completion requests (M2+).
#[derive(Debug, Clone)]
pub struct Prompt {
    pub system: Option<String>,
    pub user: String,
}

/// Pluggable embedding provider (fastembed in M2).
#[async_trait]
pub trait Embedder: Send + Sync {
    fn id(&self) -> &str;
    fn dim(&self) -> usize;
    async fn embed(&self, texts: &[String]) -> anyhow::Result<Vec<Vec<f32>>>;
}

/// Pluggable LLM completion provider (Ollama/cloud in M5).
#[async_trait]
pub trait Completer: Send + Sync {
    async fn complete(&self, prompt: &Prompt) -> anyhow::Result<String>;
}

/// Build an embedder from config. `"none"` returns an error (semantic search requires an embedder).
pub fn build_embedder(config: &Config) -> Result<Box<dyn Embedder>> {
    match config.providers.embedder.as_str() {
        "fastembed" => Ok(Box::new(FastEmbedder::new(config)?)),
        "hash" => Ok(Box::new(HashEmbedder)),
        "none" => Err(AlexandriaError::Config(
            "embedder is \"none\"; set providers.embedder to \"fastembed\" or \"hash\"".into(),
        )),
        other => Err(AlexandriaError::Config(format!(
            "unknown embedder provider: {other}"
        ))),
    }
}

/// Synchronous embed helper (blocks on async trait via pollster).
pub fn embed_sync(embedder: &dyn Embedder, texts: &[String]) -> anyhow::Result<Vec<Vec<f32>>> {
    pollster::block_on(embedder.embed(texts))
}

/// Local ONNX embeddings via fastembed (default for production).
pub struct FastEmbedder {
    id: String,
    dim: usize,
    model: TextEmbedding,
}

impl FastEmbedder {
    pub fn new(config: &Config) -> Result<Self> {
        let model_name = config
            .providers
            .embedding
            .model
            .as_deref()
            .unwrap_or("BGESmallENV15");
        let model = parse_embedding_model(model_name)?;
        let init = InitOptions::new(model.clone()).with_show_download_progress(true);
        let text_embedding = TextEmbedding::try_new(init)
            .map_err(|e| AlexandriaError::Config(format!("fastembed init failed: {e}")))?;
        let id = format!("fastembed:{model:?}");
        let probe = text_embedding
            .embed(vec!["probe"], None)
            .map_err(|e| AlexandriaError::Config(format!("fastembed probe failed: {e}")))?;
        let dim = probe.first().map(|v| v.len()).unwrap_or(384);
        Ok(Self {
            id,
            dim,
            model: text_embedding,
        })
    }
}

fn parse_embedding_model(name: &str) -> Result<EmbeddingModel> {
    match name {
        "BGESmallENV15" | "bge-small-en-v1.5" => Ok(EmbeddingModel::BGESmallENV15),
        "AllMiniLML6V2" | "all-MiniLM-L6-v2" => Ok(EmbeddingModel::AllMiniLML6V2),
        _ => Err(AlexandriaError::Config(format!(
            "unknown embedding model: {name}"
        ))),
    }
}

#[async_trait]
impl Embedder for FastEmbedder {
    fn id(&self) -> &str {
        &self.id
    }

    fn dim(&self) -> usize {
        self.dim
    }

    async fn embed(&self, texts: &[String]) -> anyhow::Result<Vec<Vec<f32>>> {
        let refs: Vec<&str> = texts.iter().map(String::as_str).collect();
        let embeddings = self.model.embed(refs, None)?;
        Ok(embeddings)
    }
}

/// Deterministic token-hash embedder for tests and offline use (no model download).
pub struct HashEmbedder;

const HASH_EMBEDDING_DIM: usize = 384;

impl HashEmbedder {
    fn hash_embed(text: &str) -> Vec<f32> {
        let mut vec = vec![0.0f32; HASH_EMBEDDING_DIM];
        for token in text.split_whitespace() {
            let mut h: u64 = 5381;
            for b in token.as_bytes() {
                h = h.wrapping_mul(33).wrapping_add(u64::from(*b));
            }
            let idx = (h as usize) % HASH_EMBEDDING_DIM;
            let sign = if (h >> 32) & 1 == 0 { 1.0 } else { -1.0 };
            vec[idx] += sign;
        }
        let norm: f32 = vec.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 0.0 {
            for v in &mut vec {
                *v /= norm;
            }
        }
        vec
    }
}

#[async_trait]
impl Embedder for HashEmbedder {
    fn id(&self) -> &str {
        "hash:v1"
    }

    fn dim(&self) -> usize {
        HASH_EMBEDDING_DIM
    }

    async fn embed(&self, texts: &[String]) -> anyhow::Result<Vec<Vec<f32>>> {
        Ok(texts.iter().map(|t| Self::hash_embed(t)).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_embedder_is_deterministic() {
        let e = HashEmbedder;
        let a = pollster::block_on(e.embed(&["hello world".into()])).unwrap();
        let b = pollster::block_on(e.embed(&["hello world".into()])).unwrap();
        assert_eq!(a[0], b[0]);
        assert_eq!(a[0].len(), HASH_EMBEDDING_DIM);
    }

    #[test]
    fn build_hash_embedder_from_config() {
        let mut config = Config::default();
        config.providers.embedder = "hash".into();
        let embedder = build_embedder(&config).unwrap();
        assert_eq!(embedder.id(), "hash:v1");
        assert_eq!(embedder.dim(), HASH_EMBEDDING_DIM);
    }
}
