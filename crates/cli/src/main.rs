mod commands;

use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};
use commands::{
    archive, consolidate, expand, forget, init, link, meta, recall, reflect, remember, reindex,
    style, threads, timeline, trace,
};

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
        /// Surfacing trigger for open threads (repeatable), e.g. topic:pricing
        #[arg(long = "surface-when")]
        surface_when: Vec<String>,
    },
    /// Hybrid fused retrieval (lexical + semantic, RRF fusion)
    Recall {
        query: String,
        #[arg(long)]
        budget: Option<u32>,
        #[arg(long)]
        audit: bool,
        #[arg(long)]
        high_stakes: bool,
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
    /// List open threads (unresolved_by_design)
    Threads {
        #[arg(long)]
        surface_for: Option<String>,
    },
    /// Relational generation parameters (never quotable bodies)
    Style {
        #[arg(long)]
        profile: bool,
    },
    /// Inspect meta-memory reliability and outcomes
    Meta {
        domain: Option<String>,
        #[arg(long)]
        record_correction: bool,
        #[arg(long)]
        correction_domain: Option<String>,
        /// Record a recall gap outcome for meta-memory (requires --gap-kind)
        #[arg(long)]
        record_gap: bool,
        /// Gap kind when recording: high_confidence_gap or low_confidence_gap
        #[arg(long)]
        gap_kind: Option<String>,
        /// Gap was warranted (not a false positive); default records as false positive
        #[arg(long)]
        gap_confirmed: bool,
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
            surface_when,
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
            surface_when,
        }),
        Commands::Recall {
            query,
            budget,
            audit,
            high_stakes,
        } => recall::run(cli.library, cli.format, query, budget, audit, high_stakes),
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
        Commands::Threads { surface_for } => {
            threads::run(cli.library, cli.format, surface_for)
        }
        Commands::Style { profile } => style::run(cli.library, cli.format, profile),
        Commands::Meta {
            domain,
            record_correction,
            correction_domain,
            record_gap,
            gap_kind,
            gap_confirmed,
        } => meta::run(meta::MetaOptions {
            library_path: cli.library,
            format: cli.format,
            domain,
            record_correction,
            correction_domain,
            record_gap,
            gap_kind,
            gap_confirmed,
        }),
    }
}
