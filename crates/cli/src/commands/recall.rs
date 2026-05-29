use std::path::PathBuf;

use alexandria_core::{Config, Index, Library, RecallOptions, Retrieval, dominant_facet};
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
    let domain = None; // populated from detected facets inside recall when auto_facet matches
    let result = retrieval.recall(
        &query,
        budget,
        RecallOptions {
            audit,
            high_stakes,
            domain,
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
    if !result.detected_facets.is_empty() {
        println!("detected_facets:");
        for f in &result.detected_facets {
            let kind = match f.kind {
                alexandria_core::FacetKind::Collection => "collection",
                alexandria_core::FacetKind::Tag => "tag",
            };
            println!("  {kind}: {} ({}) — pass --{kind} {} to scope", f.name, f.count, f.name);
        }
        if let Some(dominant) = dominant_facet(&result.detected_facets) {
            println!(
                "  hint: matched `{}`; scoped recall available via --{} {}",
                dominant.name,
                match dominant.kind {
                    alexandria_core::FacetKind::Collection => "collection",
                    alexandria_core::FacetKind::Tag => "tag",
                },
                dominant.name
            );
        }
    }
    if result.tree.collections.is_empty() {
        println!("(no matches)");
        return;
    }
    for collection in &result.tree.collections {
        println!();
        println!("## {}", collection.summary);
        for hit in &collection.hits {
            print!(
                "  [{}] {} (score: {:.4}, ~{} tokens, signals: {})",
                hit.id,
                hit.claim,
                hit.score,
                hit.token_cost,
                hit.signals.join("+")
            );
            if let Some(w) = &hit.freshness_warning {
                print!(" ⚠ {w}");
            }
            println!();
        }
    }
}
