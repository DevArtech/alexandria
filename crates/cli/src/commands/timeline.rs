use std::path::PathBuf;

use alexandria_core::{Graph, Index, Library, Tier};
use anyhow::Result;

use crate::OutputFormat;

pub fn run(
    library_path: Option<PathBuf>,
    format: OutputFormat,
    since: Option<String>,
    until: Option<String>,
    tier: Option<String>,
) -> Result<()> {
    let library = match library_path {
        Some(p) => Library::discover(Some(&p))?,
        None => Library::discover(None)?,
    };
    let index = Index::open_readonly(&library)?;
    let graph = Graph::new(&index);
    let tier = tier
        .as_deref()
        .map(Tier::parse)
        .transpose()?;
    let result = graph.timeline(since.as_deref(), until.as_deref(), tier)?;

    match format {
        OutputFormat::Human => {
            println!("{} entries", result.count);
            for entry in &result.entries {
                println!(
                    "[{}] {} ({}, {}) @ {}",
                    entry.id, entry.claim, entry.tier, entry.status, entry.created
                );
            }
        }
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&result)?),
    }
    Ok(())
}
