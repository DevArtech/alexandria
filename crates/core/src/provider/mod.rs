mod anthropic;
mod http;
mod ollama;
mod openai;

use fastembed::{EmbeddingModel, InitOptions, RerankInitOptions, RerankerModel, TextEmbedding, TextRerank};

use crate::config::Config;
use crate::error::{AlexandriaError, Result};

pub use anthropic::AnthropicCompleter;
pub use ollama::{OllamaCompleter, OllamaEmbedder};
pub use openai::{OpenAiCompleter, OpenAiEmbedder};

/// Prompt wrapper for LLM completion requests.
#[derive(Debug, Clone)]
pub struct Prompt {
    pub system: Option<String>,
    pub user: String,
}

/// Pluggable embedding provider.
///
/// All implementations use synchronous I/O so they can be called from any context without
/// risk of a nested-runtime panic. The `async_trait` indirection previously used here was
/// purely decorative — every impl used `reqwest::blocking` internally.
pub trait Embedder: Send + Sync {
    fn id(&self) -> &str;
    fn dim(&self) -> usize;
    fn embed(&self, texts: &[String]) -> anyhow::Result<Vec<Vec<f32>>>;
}

/// Pluggable LLM completion provider (synchronous, same rationale as `Embedder`).
pub trait Completer: Send + Sync {
    fn complete(&self, prompt: &Prompt) -> anyhow::Result<String>;
}

/// Optional cross-encoder reranker (fastembed local default).
pub trait Reranker: Send + Sync {
    fn rerank(&self, query: &str, docs: &[String]) -> Result<Vec<f32>>;
}

/// Build an embedder from config.
pub fn build_embedder(config: &Config) -> Result<Box<dyn Embedder>> {
    build_embedder_with_dim_hint(config, None)
}

/// Build an embedder with an optional known dim, skipping the probe call on HTTP providers.
/// `known_dim` is only honoured when the stored embedder id matches what we would build.
pub fn build_embedder_with_dim_hint(
    config: &Config,
    known_dim: Option<usize>,
) -> Result<Box<dyn Embedder>> {
    match config.providers.embedder.as_str() {
        "fastembed" => Ok(Box::new(FastEmbedder::new(config)?)),
        "hash" => Ok(Box::new(HashEmbedder)),
        "ollama" => Ok(Box::new(OllamaEmbedder::new_with_dim_hint(config, known_dim)?)),
        "openai" => Ok(Box::new(OpenAiEmbedder::new_with_dim_hint(config, known_dim)?)),
        "none" => Err(AlexandriaError::Config(
            "embedder is \"none\"; set providers.embedder to \"fastembed\", \"hash\", \"ollama\", or \"openai\"".into(),
        )),
        other => Err(AlexandriaError::Config(format!(
            "unknown embedder provider: {other}"
        ))),
    }
}

/// Predict the embedder ID that would be produced by `build_embedder` for a given config,
/// without actually constructing the embedder. Returns `None` for providers (fastembed) whose
/// ID depends on instantiation state and is cheap to probe anyway.
pub fn predict_embedder_id(config: &Config) -> Option<String> {
    Some(match config.providers.embedder.as_str() {
        "openai" => format!("openai:{}", config.providers.openai.embed_model),
        "ollama" => format!("ollama:{}", config.providers.ollama.embed_model),
        "hash" => "hash:v1".to_string(),
        _ => return None, // fastembed ID includes model Debug repr; probe is local and free
    })
}

/// Build a completer from config. Returns `None` when unset or `"none"`.
pub fn build_completer(config: &Config) -> Result<Option<Box<dyn Completer>>> {
    let name = match config.providers.completer.as_deref() {
        None | Some("") | Some("none") => return Ok(None),
        Some(s) => s,
    };
    let completer: Box<dyn Completer> = match name {
        "ollama" => Box::new(OllamaCompleter::new(config)?),
        "openai" => Box::new(OpenAiCompleter::new(config)?),
        "anthropic" => Box::new(AnthropicCompleter::new(config)?),
        other => {
            return Err(AlexandriaError::Config(format!(
                "unknown completer provider: {other}"
            )));
        }
    };
    Ok(Some(completer))
}

/// Build a reranker from config. Returns `None` when disabled.
pub fn build_reranker(config: &Config) -> Result<Option<Box<dyn Reranker>>> {
    if !config.reranker.enabled {
        return Ok(None);
    }
    Ok(Some(Box::new(FastEmbedReranker::new(config)?)))
}

/// Embed helper — thin wrapper so callers don't need to know the trait is sync.
pub fn embed_sync(embedder: &dyn Embedder, texts: &[String]) -> anyhow::Result<Vec<Vec<f32>>> {
    embedder.embed(texts)
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

impl Embedder for FastEmbedder {
    fn id(&self) -> &str {
        &self.id
    }

    fn dim(&self) -> usize {
        self.dim
    }

    fn embed(&self, texts: &[String]) -> anyhow::Result<Vec<Vec<f32>>> {
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

impl Embedder for HashEmbedder {
    fn id(&self) -> &str {
        "hash:v1"
    }

    fn dim(&self) -> usize {
        HASH_EMBEDDING_DIM
    }

    fn embed(&self, texts: &[String]) -> anyhow::Result<Vec<Vec<f32>>> {
        Ok(texts.iter().map(|t| Self::hash_embed(t)).collect())
    }
}

/// Local cross-encoder reranker via fastembed.
pub struct FastEmbedReranker {
    model: TextRerank,
}

impl FastEmbedReranker {
    pub fn new(config: &Config) -> Result<Self> {
        let model_name = parse_reranker_model(&config.reranker.model)?;
        let init = RerankInitOptions::new(model_name).with_show_download_progress(true);
        let model = TextRerank::try_new(init)
            .map_err(|e| AlexandriaError::Config(format!("fastembed reranker init failed: {e}")))?;
        Ok(Self { model })
    }
}

fn parse_reranker_model(name: &str) -> Result<RerankerModel> {
    match name {
        "JINARerankerV1TurboEn" | "jina-reranker-v1-turbo-en" => Ok(RerankerModel::JINARerankerV1TurboEn),
        "BGERerankerBase" | "bge-reranker-base" => Ok(RerankerModel::BGERerankerBase),
        "BGERerankerV2M3" | "bge-reranker-v2-m3" => Ok(RerankerModel::BGERerankerV2M3),
        "JINARerankerV2BaseMultiligual" | "jina-reranker-v2-base-multilingual" => {
            Ok(RerankerModel::JINARerankerV2BaseMultiligual)
        }
        _ => Err(AlexandriaError::Config(format!(
            "unknown reranker model: {name}"
        ))),
    }
}

impl Reranker for FastEmbedReranker {
    fn rerank(&self, query: &str, docs: &[String]) -> Result<Vec<f32>> {
        if docs.is_empty() {
            return Ok(Vec::new());
        }
        let doc_refs: Vec<&str> = docs.iter().map(String::as_str).collect();
        let results = self
            .model
            .rerank(query, doc_refs, false, None)
            .map_err(|e| AlexandriaError::Provider(format!("reranker failed: {e}")))?;
        let mut scores = vec![0.0f32; docs.len()];
        for result in results {
            if result.index < scores.len() {
                scores[result.index] = result.score;
            }
        }
        Ok(scores)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_embedder_is_deterministic() {
        let e = HashEmbedder;
        let a = e.embed(&["hello world".into()]).unwrap();
        let b = e.embed(&["hello world".into()]).unwrap();
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

    #[test]
    fn build_completer_none_by_default() {
        let config = Config::default();
        let completer = build_completer(&config).unwrap();
        assert!(completer.is_none());
    }

    #[test]
    fn build_completer_none_explicit() {
        let mut config = Config::default();
        config.providers.completer = Some("none".into());
        let completer = build_completer(&config).unwrap();
        assert!(completer.is_none());
    }

    #[test]
    fn build_reranker_disabled_by_default() {
        let config = Config::default();
        let reranker = build_reranker(&config).unwrap();
        assert!(reranker.is_none());
    }

    #[test]
    fn provider_config_defaults_present() {
        let config = Config::default();
        assert_eq!(config.providers.ollama.base_url, "http://localhost:11434");
        assert_eq!(config.providers.openai.api_key_env, "OPENAI_API_KEY");
        assert_eq!(config.providers.anthropic.api_key_env, "ANTHROPIC_API_KEY");
        assert!(!config.reranker.enabled);
        assert!(config.calibration.enabled);
    }

    #[test]
    fn predict_embedder_id_openai() {
        let mut config = Config::default();
        config.providers.embedder = "openai".into();
        let id = predict_embedder_id(&config).unwrap();
        assert!(id.starts_with("openai:"));
    }

    #[test]
    fn predict_embedder_id_ollama() {
        let mut config = Config::default();
        config.providers.embedder = "ollama".into();
        let id = predict_embedder_id(&config).unwrap();
        assert!(id.starts_with("ollama:"));
    }

    #[test]
    fn predict_embedder_id_hash() {
        let mut config = Config::default();
        config.providers.embedder = "hash".into();
        assert_eq!(predict_embedder_id(&config).unwrap(), "hash:v1");
    }

    #[test]
    fn predict_embedder_id_fastembed_none() {
        let config = Config::default(); // default embedder is "fastembed"
        assert!(predict_embedder_id(&config).is_none());
    }

    #[test]
    #[ignore = "requires running Ollama at localhost:11434"]
    fn ollama_embedder_live() {
        let mut config = Config::default();
        config.providers.embedder = "ollama".into();
        let embedder = build_embedder(&config).unwrap();
        let vecs = embedder.embed(&["hello".into()]).unwrap();
        assert_eq!(vecs[0].len(), embedder.dim());
    }

    #[test]
    #[ignore = "requires OPENAI_API_KEY"]
    fn openai_embedder_live() {
        let mut config = Config::default();
        config.providers.embedder = "openai".into();
        let embedder = build_embedder(&config).unwrap();
        let vecs = embedder.embed(&["hello".into()]).unwrap();
        assert_eq!(vecs[0].len(), embedder.dim());
    }
}
