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

pub fn check_response(provider: &str, status: StatusCode, body: &str) -> Result<()> {
    if status.is_success() {
        return Ok(());
    }
    Err(AlexandriaError::Provider(format!(
        "{provider} API error ({status}): {body}"
    )))
}

pub fn parse_json_response<T: serde::de::DeserializeOwned>(
    provider: &str,
    status: StatusCode,
    body: &str,
) -> Result<T> {
    check_response(provider, status, body)?;
    serde_json::from_str(body).map_err(|e| {
        AlexandriaError::Provider(format!("{provider} returned invalid JSON: {e}; body: {body}"))
    })
}
