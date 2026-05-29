use std::path::PathBuf;

use alexandria_core::{Config, Index, Library, Retrieval};
use anyhow::Result;

use crate::OutputFormat;
use crate::commands::util::parse_rel_cli;

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

fn print_human(result: &alexandria_core::ExpandResult) {
    println!("[{}] {}", result.id, result.claim);
    println!("tier: {}", result.tier);
    println!("status: {}", result.status);
    println!(
        "confidence: {:.2} (effective: {:.2})",
        result.confidence, result.effective_confidence
    );
    println!("token_cost: {}", result.token_cost);
    if let Some(w) = &result.freshness_warning {
        println!("freshness: {w}");
    }
    if !result.sources.is_empty() {
        println!("sources:");
        for s in &result.sources {
            if let Some(age) = s.age_days {
                println!(
                    "  {}:{} observed {} ({}d ago)",
                    s.kind,
                    s.reference,
                    s.observed.as_deref().unwrap_or("?"),
                    age
                );
            } else {
                println!("  {}:{} (freshness unknown)", s.kind, s.reference);
            }
        }
    }
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
