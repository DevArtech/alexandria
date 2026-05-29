use std::path::PathBuf;

use alexandria_core::{map, Config, Index, Library, MapOptions, Rel};
use anyhow::Result;

use crate::OutputFormat;
use crate::commands::util::parse_rel_cli;

pub fn run(
    library_path: Option<PathBuf>,
    format: OutputFormat,
    seed: String,
    depth: Option<u32>,
    rels: Vec<String>,
    budget: Option<u32>,
) -> Result<()> {
    let library = match library_path {
        Some(p) => Library::discover(Some(&p))?,
        None => Library::discover(None)?,
    };
    let config = Config::load(&library.root)?;
    let index = Index::open(&library, &config)?;

    let parsed_rels = if rels.is_empty() {
        None
    } else {
        Some(
            rels.iter()
                .map(|r| parse_rel_cli(r))
                .collect::<Result<Vec<Rel>, _>>()?,
        )
    };

    let result = map(
        &seed,
        &index,
        &config,
        MapOptions {
            depth: depth.unwrap_or(2),
            rels: parsed_rels,
            budget,
        },
    )?;

    match format {
        OutputFormat::Human => print_human(&result),
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&result)?),
    }
    Ok(())
}

fn print_human(result: &alexandria_core::MapResult) {
    println!("seed: {}", result.seed);
    println!("depth: {}", result.depth);
    println!("edge_count: {}", result.edge_count);
    println!("total_tokens: {}", result.total_tokens);

    for group in &result.rel_groups {
        println!();
        println!("## {} ({} edges, ~{} tokens)", group.rel, group.edges.len(), group.token_cost);
        for edge in &group.edges {
            println!(
                "  [{}] {} --{}--> [{}] {} (depth {})",
                edge.from_id,
                edge.from_claim,
                edge.rel,
                edge.to_id,
                edge.to_claim,
                edge.depth
            );
        }
    }

    if !result.mermaid.is_empty() {
        println!();
        println!("mermaid:");
        println!("{}", result.mermaid);
    }
}
