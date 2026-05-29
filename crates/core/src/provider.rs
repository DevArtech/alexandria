use async_trait::async_trait;

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
