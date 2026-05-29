use serde::Deserialize;

use crate::config::Config;
use crate::error::{AlexandriaError, Result};
use crate::provider::http::{check_response, new_client, optional_api_key_from_env};
use crate::provider::{Completer, Embedder, Prompt};

pub struct OpenAiEmbedder {
    id: String,
    dim: usize,
    base_url: String,
    model: String,
    api_key: Option<String>,
    client: reqwest::blocking::Client,
}

pub struct OpenAiCompleter {
    base_url: String,
    model: String,
    api_key: Option<String>,
    client: reqwest::blocking::Client,
}

/// Attach bearer auth only when a key is present, so keyless local
/// OpenAI-compatible servers (Ollama, LocalAI, TEI) are not rejected.
fn maybe_bearer(
    req: reqwest::blocking::RequestBuilder,
    api_key: &Option<String>,
) -> reqwest::blocking::RequestBuilder {
    match api_key {
        Some(key) => req.bearer_auth(key),
        None => req,
    }
}

impl OpenAiEmbedder {
    pub fn new(config: &Config) -> Result<Self> {
        Self::new_with_dim_hint(config, None)
    }

    /// Construct with an optional pre-known dim to skip the billed probe request.
    /// Pass the dim stored in `index_meta` when the embedder id matches; pass `None` otherwise.
    pub fn new_with_dim_hint(config: &Config, known_dim: Option<usize>) -> Result<Self> {
        let cfg = &config.providers.openai;
        let base_url = cfg.base_url.trim_end_matches('/').to_string();
        let model = cfg.embed_model.clone();
        let api_key = optional_api_key_from_env(&cfg.api_key_env);
        let client = new_client()?;
        let id = format!("openai:{model}");
        let dim = match known_dim {
            Some(d) => d,
            None => Self::probe_dim(&client, &base_url, &model, &api_key)?,
        };
        Ok(Self {
            id,
            dim,
            base_url,
            model,
            api_key,
            client,
        })
    }

    fn probe_dim(
        client: &reqwest::blocking::Client,
        base_url: &str,
        model: &str,
        api_key: &Option<String>,
    ) -> Result<usize> {
        let vectors = Self::embed_batch(client, base_url, model, api_key, &["probe".to_string()])?;
        vectors
            .first()
            .map(|v| v.len())
            .ok_or_else(|| AlexandriaError::Provider("openai embed probe returned no vectors".into()))
    }

    fn embed_batch(
        client: &reqwest::blocking::Client,
        base_url: &str,
        model: &str,
        api_key: &Option<String>,
        texts: &[String],
    ) -> Result<Vec<Vec<f32>>> {
        let url = format!("{base_url}/embeddings");
        let body = serde_json::json!({
            "model": model,
            "input": texts,
        });
        let response = maybe_bearer(client.post(&url), api_key)
            .json(&body)
            .send()
            .map_err(|e| AlexandriaError::Provider(format!("openai request failed: {e}")))?;
        let status = response.status();
        let text_body = response
            .text()
            .map_err(|e| AlexandriaError::Provider(format!("openai read body failed: {e}")))?;
        let parsed: OpenAiEmbeddingResponse =
            crate::provider::http::parse_json_response("openai", status, &text_body)?;
        let mut out = vec![Vec::new(); texts.len()];
        for item in parsed.data {
            if item.index < out.len() {
                out[item.index] = item.embedding;
            }
        }
        Ok(out)
    }
}

#[derive(Debug, Deserialize)]
struct OpenAiEmbeddingResponse {
    data: Vec<OpenAiEmbeddingItem>,
}

#[derive(Debug, Deserialize)]
struct OpenAiEmbeddingItem {
    index: usize,
    embedding: Vec<f32>,
}

impl Embedder for OpenAiEmbedder {
    fn id(&self) -> &str {
        &self.id
    }

    fn dim(&self) -> usize {
        self.dim
    }

    fn embed(&self, texts: &[String]) -> anyhow::Result<Vec<Vec<f32>>> {
        Ok(Self::embed_batch(
            &self.client,
            &self.base_url,
            &self.model,
            &self.api_key,
            texts,
        )?)
    }
}

impl OpenAiCompleter {
    pub fn new(config: &Config) -> Result<Self> {
        let cfg = &config.providers.openai;
        Ok(Self {
            base_url: cfg.base_url.trim_end_matches('/').to_string(),
            model: cfg.complete_model.clone(),
            api_key: optional_api_key_from_env(&cfg.api_key_env),
            client: new_client()?,
        })
    }
}

#[derive(Debug, Deserialize)]
struct OpenAiChatResponse {
    choices: Vec<OpenAiChatChoice>,
}

#[derive(Debug, Deserialize)]
struct OpenAiChatChoice {
    message: OpenAiChatMessage,
}

#[derive(Debug, Deserialize)]
struct OpenAiChatMessage {
    content: String,
}

impl Completer for OpenAiCompleter {
    fn complete(&self, prompt: &Prompt) -> anyhow::Result<String> {
        let url = format!("{}/chat/completions", self.base_url);
        let mut messages = Vec::new();
        if let Some(system) = &prompt.system {
            messages.push(serde_json::json!({"role": "system", "content": system}));
        }
        messages.push(serde_json::json!({"role": "user", "content": prompt.user}));

        let body = serde_json::json!({
            "model": self.model,
            "messages": messages,
        });
        let response = maybe_bearer(self.client.post(&url), &self.api_key)
            .json(&body)
            .send()
            .map_err(|e| AlexandriaError::Provider(format!("openai chat failed: {e}")))?;
        let status = response.status();
        let text_body = response
            .text()
            .map_err(|e| AlexandriaError::Provider(format!("openai read body failed: {e}")))?;
        check_response("openai", status, &text_body)?;
        let parsed: OpenAiChatResponse = serde_json::from_str(&text_body).map_err(|e| {
            AlexandriaError::Provider(format!("openai invalid JSON: {e}; body: {text_body}"))
        })?;
        parsed
            .choices
            .into_iter()
            .next()
            .map(|c| c.message.content)
            .ok_or_else(|| AlexandriaError::Provider("openai chat returned no choices".into()).into())
    }
}
