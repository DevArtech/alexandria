use serde::Deserialize;

use crate::config::Config;
use crate::error::{AlexandriaError, Result};
use crate::provider::http::{check_response, new_client};
use crate::provider::{Completer, Embedder, Prompt};

pub struct OllamaEmbedder {
    id: String,
    dim: usize,
    base_url: String,
    model: String,
    client: reqwest::blocking::Client,
}

pub struct OllamaCompleter {
    base_url: String,
    model: String,
    client: reqwest::blocking::Client,
}

impl OllamaEmbedder {
    pub fn new(config: &Config) -> Result<Self> {
        Self::new_with_dim_hint(config, None)
    }

    /// Construct with an optional pre-known dim to skip the probe request.
    /// Pass the dim stored in `index_meta` when the embedder id matches; pass `None` otherwise.
    pub fn new_with_dim_hint(config: &Config, known_dim: Option<usize>) -> Result<Self> {
        let cfg = &config.providers.ollama;
        let base_url = cfg.base_url.trim_end_matches('/').to_string();
        let model = cfg.embed_model.clone();
        let client = new_client()?;
        let id = format!("ollama:{model}");
        let dim = match known_dim {
            Some(d) => d,
            None => Self::probe_dim(&client, &base_url, &model)?,
        };
        Ok(Self {
            id,
            dim,
            base_url,
            model,
            client,
        })
    }

    fn probe_dim(client: &reqwest::blocking::Client, base_url: &str, model: &str) -> Result<usize> {
        let embedding = Self::embed_one(client, base_url, model, "probe")?;
        if embedding.is_empty() {
            return Err(AlexandriaError::Provider(
                "ollama embed probe returned empty vector".into(),
            ));
        }
        Ok(embedding.len())
    }

    fn embed_one(
        client: &reqwest::blocking::Client,
        base_url: &str,
        model: &str,
        text: &str,
    ) -> Result<Vec<f32>> {
        let url = format!("{base_url}/api/embeddings");
        let body = serde_json::json!({
            "model": model,
            "prompt": text,
        });
        let response = client
            .post(&url)
            .json(&body)
            .send()
            .map_err(|e| AlexandriaError::Provider(format!("ollama request failed: {e}")))?;
        let status = response.status();
        let text_body = response
            .text()
            .map_err(|e| AlexandriaError::Provider(format!("ollama read body failed: {e}")))?;
        let parsed: OllamaEmbeddingResponse =
            crate::provider::http::parse_json_response("ollama", status, &text_body)?;
        Ok(parsed.embedding)
    }
}

#[derive(Debug, Deserialize)]
struct OllamaEmbeddingResponse {
    embedding: Vec<f32>,
}

impl Embedder for OllamaEmbedder {
    fn id(&self) -> &str {
        &self.id
    }

    fn dim(&self) -> usize {
        self.dim
    }

    fn embed(&self, texts: &[String]) -> anyhow::Result<Vec<Vec<f32>>> {
        let mut out = Vec::with_capacity(texts.len());
        for text in texts {
            out.push(Self::embed_one(
                &self.client,
                &self.base_url,
                &self.model,
                text,
            )?);
        }
        Ok(out)
    }
}

impl OllamaCompleter {
    pub fn new(config: &Config) -> Result<Self> {
        let cfg = &config.providers.ollama;
        Ok(Self {
            base_url: cfg.base_url.trim_end_matches('/').to_string(),
            model: cfg.complete_model.clone(),
            client: new_client()?,
        })
    }
}

#[derive(Debug, Deserialize)]
struct OllamaChatResponse {
    message: OllamaChatMessage,
}

#[derive(Debug, Deserialize)]
struct OllamaChatMessage {
    content: String,
}

impl Completer for OllamaCompleter {
    fn complete(&self, prompt: &Prompt) -> anyhow::Result<String> {
        let url = format!("{}/api/chat", self.base_url);
        let mut messages = Vec::new();
        if let Some(system) = &prompt.system {
            messages.push(serde_json::json!({"role": "system", "content": system}));
        }
        messages.push(serde_json::json!({"role": "user", "content": prompt.user}));

        let body = serde_json::json!({
            "model": self.model,
            "messages": messages,
            "stream": false,
        });
        let response = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .map_err(|e| AlexandriaError::Provider(format!("ollama chat failed: {e}")))?;
        let status = response.status();
        let text_body = response
            .text()
            .map_err(|e| AlexandriaError::Provider(format!("ollama read body failed: {e}")))?;
        check_response("ollama", status, &text_body)?;
        let parsed: OllamaChatResponse = serde_json::from_str(&text_body).map_err(|e| {
            AlexandriaError::Provider(format!("ollama invalid JSON: {e}; body: {text_body}"))
        })?;
        Ok(parsed.message.content)
    }
}
