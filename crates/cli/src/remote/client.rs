use std::sync::OnceLock;

use anyhow::{bail, Context, Result};
use rmcp::model::{CallToolRequestParams, CallToolResult};
use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
use rmcp::transport::StreamableHttpClientTransport;
use rmcp::ServiceExt;
use serde::de::DeserializeOwned;
use serde_json::Value;
use tokio::runtime::Runtime;

use super::config::{read_token, ResolvedRemote};

static RUNTIME: OnceLock<Runtime> = OnceLock::new();

fn runtime() -> &'static Runtime {
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("failed to build Tokio runtime for remote CLI")
    })
}

fn remote_label(remote: &ResolvedRemote) -> String {
    remote
        .profile_name
        .as_deref()
        .map(|n| format!("profile '{n}'"))
        .unwrap_or_else(|| remote.mcp_url.clone())
}

pub fn call_tool(remote: &ResolvedRemote, name: &str, arguments: Value) -> Result<Value> {
    let token = read_token(&remote.token_env)?;
    let mcp_url = remote.mcp_url.clone();
    let name = name.to_string();
    let label = remote_label(remote);

    runtime().block_on(async move {
        let config = StreamableHttpClientTransportConfig::with_uri(mcp_url).auth_header(token);
        let transport = StreamableHttpClientTransport::from_config(config);

        let client = ()
            .serve(transport)
            .await
            .map_err(|e| map_init_error(e, &label))
            .with_context(|| format!("failed to connect to remote Alexandria ({label})"))?;

        let args_map = match arguments {
            Value::Object(map) if !map.is_empty() => Some(map),
            Value::Object(_) => None,
            other if other.is_null() => None,
            other => bail!("internal error: tool arguments must be a JSON object, got {other}"),
        };

        let result = client
            .call_tool(CallToolRequestParams {
                meta: None,
                name: name.into(),
                arguments: args_map,
                task: None,
            })
            .await
            .map_err(|e| map_tool_error(e, &label))?;

        client.cancel().await.ok();
        tool_result_to_value(result)
    })
}

pub fn call_and_parse<T: DeserializeOwned>(
    remote: &ResolvedRemote,
    name: &str,
    arguments: Value,
) -> Result<T> {
    let value = call_tool(remote, name, arguments)?;
    serde_json::from_value(value).with_context(|| format!("parse remote `{name}` response"))
}

fn tool_result_to_value(result: CallToolResult) -> Result<Value> {
    if result.is_error.unwrap_or(false) {
        let msg = result
            .content
            .first()
            .and_then(|c| c.raw.as_text())
            .map(|t| t.text.as_str())
            .unwrap_or("remote tool returned an error");
        bail!("remote tool error: {msg}");
    }

    let text = result
        .content
        .iter()
        .find_map(|c| c.raw.as_text())
        .map(|t| t.text.as_str())
        .context("remote tool returned no text content")?;

    serde_json::from_str(text).with_context(|| "remote tool returned invalid JSON")
}

fn map_init_error(e: rmcp::service::ClientInitializeError, label: &str) -> anyhow::Error {
    let msg = e.to_string();
    if msg.contains("401") || msg.contains("Unauthorized") || msg.contains("403") {
        anyhow::anyhow!(
            "authentication failed connecting to {label} ({msg}). \
             Check that your bearer token env var is set and matches the server/proxy."
        )
    } else {
        anyhow::anyhow!("{label}: {msg}")
    }
}

fn map_tool_error(e: rmcp::service::ServiceError, label: &str) -> anyhow::Error {
    let msg = e.to_string();
    if msg.contains("401") || msg.contains("Unauthorized") {
        anyhow::anyhow!(
            "authentication failed on {label} ({msg}). \
             Verify the bearer token in the configured token_env variable."
        )
    } else {
        anyhow::anyhow!("{label}: {msg}")
    }
}
