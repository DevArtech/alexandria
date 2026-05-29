use std::path::PathBuf;
use std::sync::Arc;

use crate::handlers::{
    archive, consolidate, expand, link, meta, recall, remember, style, threads, timeline, trace,
    ServerState,
};
use crate::params::{
    ConsolidateParams, ExpandParams, IdParams, LinkParams, MetaParams, RecallParams,
    RememberParams, ThreadsParams, TimelineParams,
};
use anyhow::Result;
use rmcp::{
    handler::server::wrapper::Parameters,
    model::{CallToolResult, Content, ServerCapabilities, ServerInfo},
    tool, tool_handler, tool_router,
    ErrorData as McpError, ServerHandler,
};
use tokio::sync::Mutex;

#[derive(Clone)]
pub struct AlexandriaMcpServer {
    state: Arc<Mutex<ServerState>>,
    tool_router: rmcp::handler::server::tool::ToolRouter<Self>,
}

impl AlexandriaMcpServer {
    pub fn new(library_path: Option<PathBuf>) -> Result<Self> {
        Ok(Self {
            state: Arc::new(Mutex::new(ServerState::open(library_path)?)),
            tool_router: Self::tool_router(),
        })
    }

    async fn json_result<T: serde::Serialize>(value: &T) -> Result<CallToolResult, McpError> {
        let text = serde_json::to_string_pretty(value)
            .map_err(|e| McpError::internal_error(format!("serialization failed: {e}"), None))?;
        Ok(CallToolResult::success(vec![Content::text(text)]))
    }

    async fn run<F, T>(&self, f: F) -> Result<CallToolResult, McpError>
    where
        F: FnOnce(&mut ServerState) -> Result<T>,
        T: serde::Serialize,
    {
        let mut guard = self.state.lock().await;
        match f(&mut guard) {
            Ok(value) => Self::json_result(&value).await,
            Err(err) => Err(McpError::invalid_params(err.to_string(), None)),
        }
    }

    async fn run_read<F, T>(&self, f: F) -> Result<CallToolResult, McpError>
    where
        F: FnOnce(&ServerState) -> Result<T>,
        T: serde::Serialize,
    {
        let guard = self.state.lock().await;
        match f(&guard) {
            Ok(value) => Self::json_result(&value).await,
            Err(err) => Err(McpError::invalid_params(err.to_string(), None)),
        }
    }
}

#[tool_router]
impl AlexandriaMcpServer {
    #[tool(description = "Hybrid fused retrieval over Alexandria memory (lexical + semantic + graph + temporal). Returns five-state recall with response_mode and a budget-aware context tree.")]
    async fn recall(
        &self,
        Parameters(params): Parameters<RecallParams>,
    ) -> Result<CallToolResult, McpError> {
        self.run_read(|state| recall(state, params)).await
    }

    #[tool(description = "Expand an engram to full body and linked claims. Relational memory is structurally suppressed.")]
    async fn expand(
        &self,
        Parameters(params): Parameters<ExpandParams>,
    ) -> Result<CallToolResult, McpError> {
        self.run_read(|state| expand(state, params)).await
    }

    #[tool(description = "Write a new engram. First line is the claim; remaining lines are the body.")]
    async fn remember(
        &self,
        Parameters(params): Parameters<RememberParams>,
    ) -> Result<CallToolResult, McpError> {
        self.run(|state| remember(state, params)).await
    }

    #[tool(description = "Create a typed edge between two engrams (supports, conflicts_confirmed, supersedes, etc.).")]
    async fn link(
        &self,
        Parameters(params): Parameters<LinkParams>,
    ) -> Result<CallToolResult, McpError> {
        self.run_read(|state| link(state, params)).await
    }

    #[tool(description = "Walk provenance back to first-party sources for an engram.")]
    async fn trace(
        &self,
        Parameters(params): Parameters<IdParams>,
    ) -> Result<CallToolResult, McpError> {
        self.run_read(|state| trace(state, params)).await
    }

    #[tool(description = "Episodic timeline view over engrams, optionally filtered by date range and tier.")]
    async fn timeline(
        &self,
        Parameters(params): Parameters<TimelineParams>,
    ) -> Result<CallToolResult, McpError> {
        self.run_read(|state| timeline(state, params)).await
    }

    #[tool(description = "List open threads (unresolved_by_design engrams), optionally filtered by surface trigger.")]
    async fn threads(
        &self,
        Parameters(params): Parameters<ThreadsParams>,
    ) -> Result<CallToolResult, McpError> {
        self.run_read(|state| threads(state, params)).await
    }

    #[tool(description = "Relational generation parameters for tone and pacing. Never quotable bodies — numeric profile only.")]
    async fn style(&self) -> Result<CallToolResult, McpError> {
        self.run_read(style).await
    }

    #[tool(description = "Inspect meta-memory reliability, or record a correction/gap outcome.")]
    async fn meta(
        &self,
        Parameters(params): Parameters<MetaParams>,
    ) -> Result<CallToolResult, McpError> {
        self.run(|state| meta(state, params)).await
    }

    #[tool(description = "Move an engram to archive (never deleted).")]
    async fn archive(
        &self,
        Parameters(params): Parameters<IdParams>,
    ) -> Result<CallToolResult, McpError> {
        self.run_read(|state| archive(state, params)).await
    }

    #[tool(description = "Run slow consolidation (dedupe, promote, decay, resummarize) or fast reflection when fast=true.")]
    async fn consolidate(
        &self,
        Parameters(params): Parameters<ConsolidateParams>,
    ) -> Result<CallToolResult, McpError> {
        self.run_read(|state| consolidate(state, params)).await
    }
}

#[tool_handler]
impl ServerHandler for AlexandriaMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            instructions: Some(
                "Alexandria memory tools. Call recall before answering; honor five-state results and response_mode; remember durable facts after acting; never quote relational memory.".into(),
            ),
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            ..Default::default()
        }
    }
}
