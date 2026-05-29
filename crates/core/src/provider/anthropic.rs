use serde::Deserialize;

use crate::config::Config;
use crate::error::{AlexandriaError, Result};
use crate::provider::http::{api_key_from_env, check_response, new_client};
use crate::provider::{Completer, Prompt};

pub struct AnthropicCompleter {
    base_url: String,
    model: String,
    api_key: String,
    client: reqwest::blocking::Client,
}

impl AnthropicCompleter {
    pub fn new(config: &Config) -> Result<Self> {
        let cfg = &config.providers.anthropic;
        Ok(Self {
            base_url: cfg.base_url.trim_end_matches('/').to_string(),
            model: cfg.complete_model.clone(),
            api_key: api_key_from_env(&cfg.api_key_env)?,
            client: new_client()?,
        })
    }
}

#[derive(Debug, Deserialize)]
struct AnthropicMessagesResponse {
    content: Vec<AnthropicContentBlock>,
}

#[derive(Debug, Deserialize)]
struct AnthropicContentBlock {
    text: Option<String>,
}

impl Completer for AnthropicCompleter {
    fn complete(&self, prompt: &Prompt) -> anyhow::Result<String> {
        let url = format!("{}/messages", self.base_url);
        let mut body = serde_json::json!({
            "model": self.model,
            "max_tokens": 4096,
            "messages": [{"role": "user", "content": prompt.user}],
        });
        if let Some(system) = &prompt.system {
            body["system"] = serde_json::Value::String(system.clone());
        }

        let response = self
            .client
            .post(&url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&body)
            .send()
            .map_err(|e| AlexandriaError::Provider(format!("anthropic request failed: {e}")))?;
        let status = response.status();
        let text_body = response
            .text()
            .map_err(|e| AlexandriaError::Provider(format!("anthropic read body failed: {e}")))?;
        check_response("anthropic", status, &text_body)?;
        let parsed: AnthropicMessagesResponse = serde_json::from_str(&text_body).map_err(|e| {
            AlexandriaError::Provider(format!("anthropic invalid JSON: {e}; body: {text_body}"))
        })?;
        let text = parsed
            .content
            .into_iter()
            .filter_map(|b| b.text)
            .collect::<Vec<_>>()
            .join("");
        if text.trim().is_empty() {
            return Err(AlexandriaError::Provider(
                "anthropic returned empty content".into(),
            )
            .into());
        }
        Ok(text)
    }
}
