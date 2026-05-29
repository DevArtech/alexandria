use std::path::PathBuf;

use alexandria_core::{Config, Index, Library, Rel, Retrieval};
use anyhow::Result;

use crate::OutputFormat;

pub fn run(
    library_path: Option<PathBuf>,
    format: OutputFormat,
    id: String,
    rel: Option<String>,
) -> Result<()> {
    let library = match library_path {
        Some(p) => Library::discover(Some(&p))?,
        None => Library::discover(None)?,
    };
    let config = Config::load(&library.root)?;
    let index = Index::open_readonly(&library)?;
    let retrieval = Retrieval::new(&index, &config);

    let rel_filter = rel
        .as_deref()
        .map(parse_rel_cli)
        .transpose()?;

    let result = retrieval.expand(&id, rel_filter)?;

    match format {
        OutputFormat::Human => print_human(&result),
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&result)?),
    }
    Ok(())
}

fn parse_rel_cli(s: &str) -> Result<Rel> {
    match s {
        "supports" => Ok(Rel::Supports),
        "refines" => Ok(Rel::Refines),
        "depends_on" => Ok(Rel::DependsOn),
        "caused_by" => Ok(Rel::CausedBy),
        "conflicts_confirmed" => Ok(Rel::ConflictsConfirmed),
        "tension_possible" => Ok(Rel::TensionPossible),
        "context_qualified" => Ok(Rel::ContextQualified),
        "coexists" => Ok(Rel::Coexists),
        "supersedes" => Ok(Rel::Supersedes),
        "superseded_by" => Ok(Rel::SupersededBy),
        "aspect_of" => Ok(Rel::AspectOf),
        "same_episode" => Ok(Rel::SameEpisode),
        other => anyhow::bail!("unknown rel: {other}"),
    }
}

fn print_human(result: &alexandria_core::ExpandResult) {
    println!("[{}] {}", result.id, result.claim);
    println!("tier: {}", result.tier);
    println!("status: {}", result.status);
    println!("token_cost: {}", result.token_cost);
    println!();
    println!("{}", result.body);
    if !result.links.is_empty() {
        println!();
        println!("links:");
        for link in &result.links {
            println!("  {} -> {} ({})", link.rel, link.to_id, link.claim);
        }
    }
}
