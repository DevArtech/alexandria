mod commands;

use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};
use commands::{init, recall, remember, reindex};

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum OutputFormat {
    Human,
    Json,
}

#[derive(Parser)]
#[command(name = "alexandria", about = "Local-first LLM memory", version)]
struct Cli {
    /// Path to library root (defaults to discovering .alexandria/ from cwd)
    #[arg(long, global = true)]
    library: Option<PathBuf>,

    #[arg(long, global = true, value_enum, default_value = "human")]
    format: OutputFormat,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize a new Alexandria library
    Init {
        /// Directory to initialize (defaults to current directory)
        path: Option<PathBuf>,
    },
    /// Write a new Engram
    Remember {
        /// Text to remember, or "-" for stdin
        text: String,
        #[arg(long)]
        tier: Option<String>,
        #[arg(long)]
        status: Option<String>,
        #[arg(long)]
        collection: Vec<String>,
        #[arg(long)]
        tag: Vec<String>,
    },
    /// Hybrid fused retrieval (lexical-only in M1)
    Recall {
        query: String,
        #[arg(long)]
        budget: Option<u32>,
    },
    /// Rebuild the SQLite index from Markdown store
    Reindex,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Init { path } => init::run(path, cli.format),
        Commands::Remember {
            text,
            tier,
            status,
            collection,
            tag,
        } => remember::run(cli.library, cli.format, text, tier, status, collection, tag),
        Commands::Recall { query, budget } => {
            recall::run(cli.library, cli.format, query, budget)
        }
        Commands::Reindex => reindex::run(cli.library, cli.format),
    }
}
