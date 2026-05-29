mod commands;

use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};
use commands::{archive, consolidate, expand, forget, init, link, recall, reflect, remember, reindex, timeline, trace};

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
        /// First-party provenance as kind:ref (repeatable), e.g. conversation:conv_2026-05-28#42
        #[arg(long)]
        source: Vec<String>,
        /// Mark as derived from another engram id (repeatable)
        #[arg(long = "derived-from")]
        derived_from: Vec<String>,
    },
    /// Hybrid fused retrieval (lexical + semantic, RRF fusion)
    Recall {
        query: String,
        #[arg(long)]
        budget: Option<u32>,
    },
    /// Expand an engram to full body and linked claims
    Expand {
        id: String,
        #[arg(long)]
        rel: Option<String>,
    },
    /// Rebuild the SQLite index from Markdown store
    Reindex,
    /// Create a typed edge between two engrams
    Link {
        from: String,
        rel: String,
        to: String,
    },
    /// Walk provenance back to first-party sources
    Trace {
        id: String,
    },
    /// Episodic view over time
    Timeline {
        #[arg(long)]
        since: Option<String>,
        #[arg(long)]
        until: Option<String>,
        #[arg(long)]
        tier: Option<String>,
    },
    /// Move an engram to archive (never deleted)
    Archive {
        id: String,
    },
    /// Alias for archive — move to archive tier
    Forget {
        id: String,
    },
    /// Slow-pass consolidation (dedupe, promote, decay, re-summarize)
    Consolidate,
    /// Slow reflection pass (same as consolidate in M3)
    Reflect {
        #[arg(long)]
        fast: bool,
    },
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
            source,
            derived_from,
        } => remember::run(remember::RememberOptions {
            library_path: cli.library,
            format: cli.format,
            text,
            tier,
            status,
            collections: collection,
            tags: tag,
            sources: source,
            derived_from,
        }),
        Commands::Recall { query, budget } => {
            recall::run(cli.library, cli.format, query, budget)
        }
        Commands::Expand { id, rel } => expand::run(cli.library, cli.format, id, rel),
        Commands::Reindex => reindex::run(cli.library, cli.format),
        Commands::Link { from, rel, to } => link::run(cli.library, cli.format, from, rel, to),
        Commands::Trace { id } => trace::run(cli.library, cli.format, id),
        Commands::Timeline { since, until, tier } => {
            timeline::run(cli.library, cli.format, since, until, tier)
        }
        Commands::Archive { id } => archive::run(cli.library, cli.format, id),
        Commands::Forget { id } => forget::run(cli.library, cli.format, id),
        Commands::Consolidate => consolidate::run(cli.library, cli.format),
        Commands::Reflect { fast } => reflect::run(cli.library, cli.format, fast),
    }
}
