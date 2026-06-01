mod commands;
mod remote;

use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};
use commands::{
    archive, catalog, consolidate, coverage, expand, forget, init, link, map, meta, pack, recall,
    reflect, reindex, remember, style, survey, threads, timeline, trace,
};
use remote::{dispatch, guard_local_only, manage, resolve_remote};

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

    /// Remote MCP server: profile name or full base URL (overrides config default)
    #[arg(long, global = true)]
    remote: Option<String>,

    /// Force local library for this command (overrides config default remote)
    #[arg(long, global = true, conflicts_with = "remote")]
    local: bool,

    /// Env var holding the bearer token for --remote (default: ALEXANDRIA_MCP_TOKEN)
    #[arg(long, global = true)]
    token_env: Option<String>,

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
        /// ISO8601 observation time applied to --source entries (defaults to now for observation kind)
        #[arg(long)]
        observed: Option<String>,
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
        /// Restrict to engrams in this collection (repeatable; structured recall)
        #[arg(long)]
        collection: Vec<String>,
        /// Restrict to engrams with this tag (repeatable; structured recall)
        #[arg(long)]
        tag: Vec<String>,
    },
    /// Expand an engram to full body and linked claims
    Expand {
        id: String,
        #[arg(long)]
        rel: Option<String>,
    },
    /// List the collections and tags memory is organized by (with counts)
    Catalog {
        /// Regenerate LIBRARY.md at the library root
        #[arg(long)]
        write_overview: bool,
    },
    /// Memory-density x-ray for a topic (counts, provenance, recency, detail ratio)
    Coverage { topic: String },
    /// Exhaustive-but-budgeted topic traversal (claims + body token costs)
    Survey {
        topic: String,
        #[arg(long)]
        budget: Option<u32>,
        #[arg(long)]
        depth: Option<u32>,
    },
    /// Concept graph / relationship map from an engram id or topic
    Map {
        /// Engram id or topic query to seed the graph
        seed: String,
        #[arg(long)]
        depth: Option<u32>,
        #[arg(long)]
        rel: Vec<String>,
        #[arg(long)]
        budget: Option<u32>,
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
    Trace { id: String },
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
    Archive { id: String },
    /// Alias for archive — move to archive tier
    Forget { id: String },
    /// Slow-pass consolidation (dedupe, promote, decay, re-summarize)
    Consolidate {
        /// Preview consolidation changes without writing
        #[arg(long)]
        dry_run: bool,
    },
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
    /// Manage remote MCP server profiles (~/.config/alexandria/remote.toml)
    Remote {
        #[command(subcommand)]
        command: RemoteCommands,
    },
    /// Export or install portable memory packs
    Pack {
        #[command(subcommand)]
        command: PackCommands,
    },
}

#[derive(Subcommand)]
enum RemoteCommands {
    /// Add or update a named remote profile
    Add {
        /// Profile name (e.g. prod, local)
        name: String,
        /// Server base URL (https://memory.example.com or http://127.0.0.1:8080)
        #[arg(long)]
        url: String,
        /// Env var holding the bearer token
        #[arg(long)]
        token_env: Option<String>,
        /// Set as the default profile
        #[arg(long)]
        default: bool,
    },
    /// List configured remote profiles
    List,
    /// Remove a remote profile
    Remove { name: String },
    /// Set the default target: profile name or `local` for on-disk library
    Use {
        /// Profile name, or `local` to use the library on disk by default
        name: String,
    },
}

#[derive(Subcommand)]
enum PackCommands {
    /// Export a curated, read-only snapshot pack from the library
    Export {
        /// Output directory for the pack (must be empty or not exist)
        #[arg(long)]
        target: PathBuf,
        /// Human-readable pack name (defaults from selectors)
        #[arg(long)]
        name: Option<String>,
        /// Include engrams in this collection (repeatable; union with tags)
        #[arg(long)]
        collection: Vec<String>,
        /// Include engrams with this tag (repeatable; union with collections)
        #[arg(long)]
        tag: Vec<String>,
        /// Include archived and superseded engrams
        #[arg(long)]
        include_archived: bool,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    if matches!(cli.command, Commands::Remote { .. }) {
        return run_remote_command(cli.format, cli.command);
    }

    let remote = if cli.local {
        None
    } else {
        resolve_remote(cli.remote.as_deref(), cli.token_env.as_deref())?
    };

    match cli.command {
        Commands::Init { path } => {
            guard_local_only(remote.as_ref(), "init")?;
            init::run(path, cli.format)
        }
        Commands::Remember {
            text,
            tier,
            status,
            collection,
            tag,
            source,
            derived_from,
            surface_when,
            observed,
        } => {
            let opts = remember::RememberOptions {
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
                observed,
            };
            if let Some(r) = remote.as_ref() {
                dispatch::remember(r, cli.format, opts)
            } else {
                remember::run(opts)
            }
        }
        Commands::Recall {
            query,
            budget,
            audit,
            high_stakes,
            collection,
            tag,
        } => {
            if let Some(r) = remote.as_ref() {
                dispatch::recall(
                    r,
                    cli.format,
                    dispatch::RecallRemoteArgs {
                        query,
                        budget,
                        audit,
                        high_stakes,
                        collections: collection,
                        tags: tag,
                    },
                )
            } else {
                recall::run(
                    cli.library,
                    cli.format,
                    query,
                    budget,
                    audit,
                    high_stakes,
                    collection,
                    tag,
                )
            }
        }
        Commands::Expand { id, rel } => {
            if let Some(r) = remote.as_ref() {
                dispatch::expand(r, cli.format, id, rel)
            } else {
                expand::run(cli.library, cli.format, id, rel)
            }
        }
        Commands::Catalog { write_overview } => {
            if let Some(r) = remote.as_ref() {
                dispatch::catalog(r, cli.format, write_overview)
            } else {
                catalog::run(cli.library, cli.format, write_overview)
            }
        }
        Commands::Coverage { topic } => {
            if let Some(r) = remote.as_ref() {
                dispatch::coverage(r, cli.format, topic)
            } else {
                coverage::run(cli.library, cli.format, topic)
            }
        }
        Commands::Survey {
            topic,
            budget,
            depth,
        } => {
            if let Some(r) = remote.as_ref() {
                dispatch::survey(r, cli.format, topic, budget, depth)
            } else {
                survey::run(cli.library, cli.format, topic, budget, depth)
            }
        }
        Commands::Map {
            seed,
            depth,
            rel,
            budget,
        } => {
            if let Some(r) = remote.as_ref() {
                dispatch::map(r, cli.format, seed, depth, rel, budget)
            } else {
                map::run(cli.library, cli.format, seed, depth, rel, budget)
            }
        }
        Commands::Reindex => {
            guard_local_only(remote.as_ref(), "reindex")?;
            reindex::run(cli.library, cli.format)
        }
        Commands::Link { from, rel, to } => {
            if let Some(r) = remote.as_ref() {
                dispatch::link(r, cli.format, from, rel, to)
            } else {
                link::run(cli.library, cli.format, from, rel, to)
            }
        }
        Commands::Trace { id } => {
            if let Some(r) = remote.as_ref() {
                dispatch::trace(r, cli.format, id)
            } else {
                trace::run(cli.library, cli.format, id)
            }
        }
        Commands::Timeline { since, until, tier } => {
            if let Some(r) = remote.as_ref() {
                dispatch::timeline(r, cli.format, since, until, tier)
            } else {
                timeline::run(cli.library, cli.format, since, until, tier)
            }
        }
        Commands::Archive { id } => {
            if let Some(r) = remote.as_ref() {
                dispatch::archive(r, cli.format, id)
            } else {
                archive::run(cli.library, cli.format, id)
            }
        }
        Commands::Forget { id } => {
            if let Some(r) = remote.as_ref() {
                dispatch::archive(r, cli.format, id)
            } else {
                forget::run(cli.library, cli.format, id)
            }
        }
        Commands::Consolidate { dry_run } => {
            if let Some(r) = remote.as_ref() {
                dispatch::consolidate(r, cli.format, dry_run)
            } else {
                consolidate::run(cli.library, cli.format, dry_run)
            }
        }
        Commands::Reflect { fast } => {
            if let Some(r) = remote.as_ref() {
                dispatch::reflect(r, cli.format, fast)
            } else {
                reflect::run(cli.library, cli.format, fast)
            }
        }
        Commands::Threads { surface_for } => {
            if let Some(r) = remote.as_ref() {
                dispatch::threads(r, cli.format, surface_for)
            } else {
                threads::run(cli.library, cli.format, surface_for)
            }
        }
        Commands::Style { profile } => {
            if let Some(r) = remote.as_ref() {
                dispatch::style(r, cli.format, profile)
            } else {
                style::run(cli.library, cli.format, profile)
            }
        }
        Commands::Meta {
            domain,
            record_correction,
            correction_domain,
            record_gap,
            gap_kind,
            gap_confirmed,
        } => {
            let opts = meta::MetaOptions {
                library_path: cli.library,
                format: cli.format,
                domain,
                record_correction,
                correction_domain,
                record_gap,
                gap_kind,
                gap_confirmed,
            };
            if let Some(r) = remote.as_ref() {
                dispatch::meta(r, cli.format, opts)
            } else {
                meta::run(opts)
            }
        }
        Commands::Remote { .. } => unreachable!(),
        Commands::Pack { command } => {
            guard_local_only(remote.as_ref(), "pack")?;
            match command {
                PackCommands::Export {
                    target,
                    name,
                    collection,
                    tag,
                    include_archived,
                } => pack::run_export(
                    cli.library,
                    cli.format,
                    target,
                    name,
                    collection,
                    tag,
                    include_archived,
                ),
            }
        }
    }
}

fn run_remote_command(format: OutputFormat, command: Commands) -> Result<()> {
    let Commands::Remote { command } = command else {
        unreachable!();
    };
    match command {
        RemoteCommands::Add {
            name,
            url,
            token_env,
            default,
        } => manage::add(name, url, token_env, default, format),
        RemoteCommands::List => manage::list(format),
        RemoteCommands::Remove { name } => manage::remove(name, format),
        RemoteCommands::Use { name } => manage::set_default(name, format),
    }
}
