use std::time::Duration;

use reqwest::blocking::Client;
use reqwest::StatusCode;

use crate::error::{AlexandriaError, Result};

pub fn new_client() -> Result<Client> {
    Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .map_err(|e| AlexandriaError::Provider(format!("failed to build HTTP client: {e}")))
}

pub fn api_key_from_env(env_var: &str) -> Result<String> {
    std::env::var(env_var).map_err(|_| {
        AlexandriaError::Provider(format!(
            "environment variable {env_var} is not set (required for this provider)"
        ))
    })
}

/// Like [`api_key_from_env`] but returns `None` when the variable is unset or
/// empty, instead of erroring. Used for OpenAI-compatible local endpoints
/// (Ollama, LocalAI, text-embeddings-inference, …) that need no credential.
pub fn optional_api_key_from_env(env_var: &str) -> Option<String> {
    match std::env::var(env_var) {
        Ok(v) if !v.trim().is_empty() => Some(v),
        _ => None,
    }
}

/// Max length of an upstream response body echoed into an error message.
/// Bodies can be large and may carry sensitive or noisy content, so we cap them
/// before they reach logs or MCP clients.
const MAX_ERR_BODY_CHARS: usize = 500;

/// Trim and truncate an upstream body for safe inclusion in an error string.
pub fn body_snippet(body: &str) -> String {
    let trimmed = body.trim();
    if trimmed.chars().count() <= MAX_ERR_BODY_CHARS {
        return trimmed.to_string();
    }
    let mut snippet: String = trimmed.chars().take(MAX_ERR_BODY_CHARS).collect();
    snippet.push_str("… [truncated]");
    snippet
}

pub fn check_response(provider: &str, status: StatusCode, body: &str) -> Result<()> {
    if status.is_success() {
        return Ok(());
    }
    Err(AlexandriaError::Provider(format!(
        "{provider} API error ({status}): {}",
        body_snippet(body)
    )))
}

pub fn parse_json_response<T: serde::de::DeserializeOwned>(
    provider: &str,
    status: StatusCode,
    body: &str,
) -> Result<T> {
    check_response(provider, status, body)?;
    serde_json::from_str(body).map_err(|e| {
        AlexandriaError::Provider(format!(
            "{provider} returned invalid JSON: {e}; body: {}",
            body_snippet(body)
        ))
    })
}
