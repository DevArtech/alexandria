use std::path::PathBuf;
use std::sync::Arc;

use alexandria_mcp::AlexandriaMcpServer;
use anyhow::{Context, Result};
use axum::{
    body::Body,
    extract::State,
    http::{header::AUTHORIZATION, Request, StatusCode},
    middleware::{self, Next},
    response::Response,
    routing::get,
    Router,
};
use clap::{Parser, ValueEnum};
use rmcp::{
    transport::{
        stdio,
        streamable_http_server::{session::local::LocalSessionManager, StreamableHttpService},
    },
    ServiceExt,
};

#[derive(Debug, Clone, Copy, ValueEnum)]
enum Transport {
    /// Local subprocess transport (one client spawns one server). Default.
    Stdio,
    /// Streamable HTTP transport for a shared, remote, multi-client server.
    Http,
}

#[derive(Parser)]
#[command(name = "alexandria-mcp", about = "Alexandria MCP server", version)]
struct Args {
    /// Path to library root (defaults to discovering .alexandria/ from cwd)
    #[arg(long)]
    library: Option<PathBuf>,

    /// Transport to serve on.
    #[arg(long, value_enum, default_value = "stdio")]
    transport: Transport,

    /// Address to bind when transport=http.
    #[arg(long, default_value = "127.0.0.1:8080")]
    bind: String,

    /// Env var holding the bearer token required on HTTP requests.
    /// If the variable is unset/empty, the HTTP server runs UNAUTHENTICATED
    /// (only do this behind a trusted reverse proxy).
    #[arg(long, default_value = "ALEXANDRIA_MCP_TOKEN")]
    auth_token_env: String,
}

fn main() -> Result<()> {
    let args = Args::parse();

    // Build the server (opens the index + constructs the embedder) OUTSIDE any
    // async runtime. HTTP-backed providers use `reqwest::blocking`, whose client
    // must not be constructed within a Tokio context, or it panics on runtime
    // drop. We build first, then enter the runtime to serve.
    let server = AlexandriaMcpServer::new(args.library.clone())?;

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("failed to build Tokio runtime")?;

    runtime.block_on(async move {
        match args.transport {
            Transport::Stdio => run_stdio(server).await,
            Transport::Http => run_http(server, args).await,
        }
    })
}

async fn run_stdio(server: AlexandriaMcpServer) -> Result<()> {
    // NB: stdio transport uses stdout for the protocol — never log to stdout here.
    let service = server.serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}

async fn run_http(server: AlexandriaMcpServer, args: Args) -> Result<()> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    // One shared server instance (single index, serialized via interior mutex).
    // The session factory hands every client a clone over the same state.
    let mcp_service = StreamableHttpService::new(
        move || Ok(server.clone()),
        Arc::new(LocalSessionManager::default()),
        Default::default(),
    );

    let token = std::env::var(&args.auth_token_env)
        .ok()
        .filter(|t| !t.trim().is_empty());

    let mut app = Router::new()
        .route("/health", get(|| async { "ok" }))
        .nest_service("/mcp", mcp_service);

    match &token {
        Some(tok) => {
            app = app.layer(middleware::from_fn_with_state(
                Arc::new(tok.clone()),
                bearer_auth,
            ));
            tracing::info!("bearer auth enabled (token from ${})", args.auth_token_env);
        }
        None => {
            tracing::warn!(
                "${} is unset — HTTP server is UNAUTHENTICATED; put it behind a trusted proxy",
                args.auth_token_env
            );
        }
    }

    let listener = tokio::net::TcpListener::bind(&args.bind)
        .await
        .with_context(|| format!("failed to bind {}", args.bind))?;
    tracing::info!("alexandria-mcp listening on http://{}/mcp", args.bind);

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("http server error")?;
    Ok(())
}

async fn bearer_auth(
    State(expected): State<Arc<String>>,
    req: Request<Body>,
    next: Next,
) -> Result<Response, StatusCode> {
    // Health checks are exempt so orchestrators can probe without the token.
    if req.uri().path() == "/health" {
        return Ok(next.run(req).await);
    }
    let presented = req
        .headers()
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("");
    if constant_time_eq(presented.as_bytes(), expected.as_bytes()) {
        Ok(next.run(req).await)
    } else {
        Err(StatusCode::UNAUTHORIZED)
    }
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}
