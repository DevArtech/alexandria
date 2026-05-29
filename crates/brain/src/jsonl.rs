use std::io::BufRead;

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryActivity {
    pub server: String,
    pub tool: String,
    pub status: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenUsage {
    pub input_tokens: u64,
    pub cached_input_tokens: u64,
    pub output_tokens: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParsedCodexRun {
    pub thread_id: Option<String>,
    pub final_message: Option<String>,
    pub memory_activity: Vec<MemoryActivity>,
    pub usage: Option<TokenUsage>,
    pub failed: bool,
    pub error_message: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ThreadStarted {
    thread_id: String,
}

#[derive(Debug, Deserialize)]
struct TurnFailed {
    error: TurnError,
}

#[derive(Debug, Deserialize)]
struct TurnError {
    message: String,
}

#[derive(Debug, Deserialize)]
struct TurnCompleted {
    usage: UsageFields,
}

#[derive(Debug, Deserialize)]
struct UsageFields {
    input_tokens: u64,
    cached_input_tokens: u64,
    output_tokens: u64,
}

#[derive(Debug, Deserialize)]
struct ItemEnvelope {
    item: ItemPayload,
}

#[derive(Debug, Deserialize)]
struct ItemPayload {
    #[serde(rename = "type")]
    item_type: String,
    text: Option<String>,
    server: Option<String>,
    tool: Option<String>,
    status: Option<String>,
    message: Option<String>,
}

pub fn parse_codex_jsonl<R: BufRead>(reader: R) -> Result<ParsedCodexRun> {
    let mut run = ParsedCodexRun {
        failed: false,
        ..Default::default()
    };

    for line in reader.lines() {
        let line = line.context("reading codex jsonl")?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let event: Value = serde_json::from_str(trimmed)
            .with_context(|| format!("invalid jsonl line: {trimmed}"))?;
        let Some(event_type) = event.get("type").and_then(Value::as_str) else {
            continue;
        };

        match event_type {
            "thread.started" => {
                let parsed: ThreadStarted = serde_json::from_value(event)?;
                run.thread_id = Some(parsed.thread_id);
            }
            "turn.completed" => {
                let parsed: TurnCompleted = serde_json::from_value(event)?;
                run.usage = Some(TokenUsage {
                    input_tokens: parsed.usage.input_tokens,
                    cached_input_tokens: parsed.usage.cached_input_tokens,
                    output_tokens: parsed.usage.output_tokens,
                });
            }
            "turn.failed" => {
                let parsed: TurnFailed = serde_json::from_value(event)?;
                run.failed = true;
                run.error_message = Some(parsed.error.message);
            }
            "error" => {
                let message = event
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown stream error");
                if message.starts_with("Reconnecting...") {
                    continue;
                }
                run.failed = true;
                run.error_message = Some(message.to_string());
            }
            "item.completed" => {
                let parsed: ItemEnvelope = serde_json::from_value(event)?;
                match parsed.item.item_type.as_str() {
                    "agent_message" => {
                        run.final_message = parsed.item.text;
                    }
                    "mcp_tool_call" => {
                        if parsed.item.server.as_deref() == Some("alexandria") {
                            run.memory_activity.push(MemoryActivity {
                                server: parsed.item.server.unwrap_or_default(),
                                tool: parsed.item.tool.unwrap_or_default(),
                                status: parsed
                                    .item
                                    .status
                                    .unwrap_or_else(|| "completed".to_string()),
                            });
                        }
                    }
                    "error" => {
                        if let Some(msg) = parsed.item.message {
                            run.error_message = Some(msg);
                        }
                    }
                    _ => {}
                }
            }
            "item.started" => {
                let parsed: ItemEnvelope = serde_json::from_value(event)?;
                if parsed.item.item_type == "mcp_tool_call"
                    && parsed.item.server.as_deref() == Some("alexandria")
                {
                    run.memory_activity.push(MemoryActivity {
                        server: parsed.item.server.unwrap_or_default(),
                        tool: parsed.item.tool.unwrap_or_default(),
                        status: parsed
                            .item
                            .status
                            .unwrap_or_else(|| "in_progress".to_string()),
                    });
                }
            }
            _ => {}
        }
    }

    if run.failed && run.final_message.is_none() {
        bail!(
            run.error_message
                .clone()
                .unwrap_or_else(|| "codex turn failed".to_string())
        );
    }

    Ok(run)
}

pub fn build_run_prompt(task: &str) -> String {
    format!("Use the $alexandria-memory skill.\n\n{task}")
}

pub fn codex_home_for_library(library_root: &std::path::Path) -> std::path::PathBuf {
    library_root.join(".alexandria").join("codex")
}

pub fn ensure_codex_config(
    library_root: &std::path::Path,
    mcp_binary: &std::path::Path,
) -> Result<std::path::PathBuf> {
    let codex_home = codex_home_for_library(library_root);
    std::fs::create_dir_all(codex_home.join("skills").join("alexandria-memory"))?;

    let config_path = codex_home.join("config.toml");
    let library_abs = library_root
        .canonicalize()
        .unwrap_or_else(|_| library_root.to_path_buf());
    let mcp_abs = mcp_binary
        .canonicalize()
        .unwrap_or_else(|_| mcp_binary.to_path_buf());

    let config = format!(
        r#"[mcp_servers.alexandria]
command = "{}"
args = ["--library", "{}"]
enabled = true
"#,
        escape_toml_string(&mcp_abs.display().to_string()),
        escape_toml_string(&library_abs.display().to_string()),
    );
    std::fs::write(&config_path, config)?;

    let skill_path = codex_home
        .join("skills")
        .join("alexandria-memory")
        .join("SKILL.md");
    std::fs::write(skill_path, crate::skill::ALEXANDRIA_MEMORY_SKILL)?;

    Ok(codex_home)
}

fn escape_toml_string(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

pub fn find_on_path(name: &str) -> Result<std::path::PathBuf> {
    let path_var = std::env::var_os("PATH").ok_or_else(|| anyhow!("PATH not set"))?;
    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    bail!("{name} not found on PATH")
}

pub fn resolve_mcp_binary() -> Result<std::path::PathBuf> {
    if let Ok(explicit) = std::env::var("ALEXANDRIA_MCP") {
        let path = std::path::PathBuf::from(explicit);
        if path.is_file() {
            return Ok(path);
        }
        bail!("ALEXANDRIA_MCP points to a missing file: {}", path.display());
    }

    if let Ok(current) = std::env::current_exe() {
        if let Some(dir) = current.parent() {
            let sibling = dir.join("alexandria-mcp");
            if sibling.is_file() {
                return Ok(sibling);
            }
        }
    }

    find_on_path("alexandria-mcp")
}

pub fn resolve_codex_binary() -> Result<std::path::PathBuf> {
    find_on_path("codex")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_final_message_and_usage() {
        let jsonl = r#"{"type":"thread.started","thread_id":"t1"}
{"type":"turn.started"}
{"type":"item.completed","item":{"id":"i1","type":"agent_message","text":"Hello from memory."}}
{"type":"turn.completed","usage":{"input_tokens":100,"cached_input_tokens":50,"output_tokens":25}}
"#;
        let run = parse_codex_jsonl(jsonl.as_bytes()).unwrap();
        assert_eq!(run.thread_id.as_deref(), Some("t1"));
        assert_eq!(run.final_message.as_deref(), Some("Hello from memory."));
        assert!(!run.failed);
        assert_eq!(
            run.usage,
            Some(TokenUsage {
                input_tokens: 100,
                cached_input_tokens: 50,
                output_tokens: 25,
            })
        );
    }

    #[test]
    fn parse_mcp_tool_calls_and_turn_failed() {
        let jsonl = r#"{"type":"item.started","item":{"id":"m1","type":"mcp_tool_call","server":"alexandria","tool":"recall","status":"in_progress"}}
{"type":"item.completed","item":{"id":"m1","type":"mcp_tool_call","server":"alexandria","tool":"recall","status":"completed"}}
{"type":"turn.failed","error":{"message":"model stream ended"}}
"#;
        let err = parse_codex_jsonl(jsonl.as_bytes()).unwrap_err();
        assert!(err.to_string().contains("model stream ended"));
    }

    #[test]
    fn build_prompt_mentions_skill() {
        let prompt = build_run_prompt("Summarize project status.");
        assert!(prompt.contains("$alexandria-memory"));
        assert!(prompt.contains("Summarize project status."));
    }
}
