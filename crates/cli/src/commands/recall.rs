use std::path::PathBuf;

use alexandria_core::{Config, Index, Library, Retrieval};
use anyhow::Result;

use crate::OutputFormat;

pub fn run(
    library_path: Option<PathBuf>,
    format: OutputFormat,
    query: String,
    budget: Option<u32>,
) -> Result<()> {
    let library = match library_path {
        Some(p) => Library::discover(Some(&p))?,
        None => Library::discover(None)?,
    };
    let config = Config::load(&library.root)?;
    let index = Index::open(&library)?;
    let retrieval = Retrieval::new(&index, &config);
    let result = retrieval.recall(&query, budget)?;

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
    if result.engrams.is_empty() {
        println!("(no matches)");
        return;
    }
    for hit in &result.engrams {
        println!(
            "  [{}] {} (score: {:.2}, ~{} tokens)",
            hit.id, hit.claim, hit.score, hit.token_cost
        );
    }
}
