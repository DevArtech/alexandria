use std::path::PathBuf;

use alexandria_core::{Config, Index, Library, RecallOptions, Retrieval};
use anyhow::Result;

use crate::OutputFormat;

#[allow(clippy::too_many_arguments)]
pub fn run(
    library_path: Option<PathBuf>,
    format: OutputFormat,
    query: String,
    budget: Option<u32>,
    audit: bool,
    high_stakes: bool,
    collections: Vec<String>,
    tags: Vec<String>,
) -> Result<()> {
    let library = match library_path {
        Some(p) => Library::discover(Some(&p))?,
        None => Library::discover(None)?,
    };
    let config = Config::load(&library.root)?;
    let index = Index::open(&library, &config)?;
    let retrieval = Retrieval::new(&index, &config);
    let result = retrieval.recall(
        &query,
        budget,
        RecallOptions {
            audit,
            high_stakes,
            domain: None,
            collections,
            tags,
        },
    )?;

    match format {
        OutputFormat::Human => print_human(&result),
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&result)?),
    }
    Ok(())
}

fn print_human(result: &alexandria_core::RecallResult) {
    println!("state: {}", result.state.as_str());
    println!("response_mode: {}", result.response_mode.as_str());
    println!("total_tokens: {}", result.total_tokens);
    if result.tree.collections.is_empty() {
        println!("(no matches)");
        return;
    }
    for collection in &result.tree.collections {
        println!();
        println!("## {}", collection.summary);
        for hit in &collection.hits {
            println!(
                "  [{}] {} (score: {:.4}, ~{} tokens, signals: {})",
                hit.id,
                hit.claim,
                hit.score,
                hit.token_cost,
                hit.signals.join("+")
            );
        }
    }
}
