use std::path::PathBuf;

use alexandria_mcp::AlexandriaMcpServer;
use anyhow::Result;
use clap::Parser;
use rmcp::{ServiceExt, transport::stdio};

#[derive(Parser)]
#[command(name = "alexandria-mcp", about = "Alexandria MCP server", version)]
struct Args {
    /// Path to library root (defaults to discovering .alexandria/ from cwd)
    #[arg(long)]
    library: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let server = AlexandriaMcpServer::new(args.library)?;
    let service = server.serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}
