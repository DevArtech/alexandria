use std::path::PathBuf;

use alexandria_core::{survey, Config, Index, Library, SurveyOptions};
use anyhow::Result;

use crate::OutputFormat;

pub fn run(
    library_path: Option<PathBuf>,
    format: OutputFormat,
    topic: String,
    budget: Option<u32>,
    depth: Option<u32>,
) -> Result<()> {
    let library = match library_path {
        Some(p) => Library::discover(Some(&p))?,
        None => Library::discover(None)?,
    };
    let config = Config::load(&library.root)?;
    let index = Index::open(&library, &config)?;
    let result = survey(
        &topic,
        &index,
        &config,
        budget,
        SurveyOptions {
            depth: depth.unwrap_or(2),
        },
    )?;

    match format {
        OutputFormat::Human => print_human(&result),
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&result)?),
    }
    Ok(())
}

fn print_human(result: &alexandria_core::SurveyResult) {
    println!("topic: {}", result.topic);
    println!("state: {}", result.state.as_str());
    println!("response_mode: {}", result.response_mode.as_str());
    println!("engram_count: {}", result.engram_count);
    println!("total_tokens: {}", result.total_tokens);

    if !result.detected_facets.is_empty() {
        println!("detected_facets:");
        for f in &result.detected_facets {
            let kind = match f.kind {
                alexandria_core::FacetKind::Collection => "collection",
                alexandria_core::FacetKind::Tag => "tag",
            };
            println!("  {kind}: {} ({})", f.name, f.count);
        }
    }

    for collection in &result.collections {
        println!();
        println!("## {}", collection.summary);
        for hit in &collection.hits {
            println!(
                "  [{}] {} (~{} claim / ~{} body tokens, expansion {:.1}x, signals: {})",
                hit.id,
                hit.claim,
                hit.claim_tokens,
                hit.body_tokens,
                hit.expansion_value,
                hit.signals.join("+")
            );
        }
    }

    if !result.gaps.is_empty() {
        println!();
        println!("gaps (touched but thin):");
        for gap in &result.gaps {
            println!("  {} {}: {}", gap.facet_kind, gap.facet_name, gap.note);
        }
    }
}
