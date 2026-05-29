use std::path::PathBuf;

use alexandria_core::{coverage, Config, Index, Library};
use anyhow::Result;

use crate::OutputFormat;

pub fn run(library_path: Option<PathBuf>, format: OutputFormat, topic: String) -> Result<()> {
    let library = match library_path {
        Some(p) => Library::discover(Some(&p))?,
        None => Library::discover(None)?,
    };
    let config = Config::load(&library.root)?;
    let index = Index::open(&library, &config)?;
    let report = coverage(&topic, &index, &config)?;

    match format {
        OutputFormat::Human => print_human(&report),
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&report)?),
    }
    Ok(())
}

fn print_human(report: &alexandria_core::CoverageReport) {
    println!("topic: {}", report.topic);
    if !report.detected_facets.is_empty() {
        println!("detected_facets:");
        for f in &report.detected_facets {
            println!(
                "  {:?} {} ({})",
                f.kind,
                f.name,
                f.count
            );
        }
    }
    println!("engram_count: {}", report.engram_count);
    println!("canonical_count: {}", report.canonical_count);
    println!("provisional_count: {}", report.provisional_count);
    println!("open_threads: {}", report.open_threads);
    println!(
        "provenance: {} total ({} first-party, {} derived, {} engrams with sources)",
        report.provenance.total_sources,
        report.provenance.first_party_sources,
        report.provenance.derived_sources,
        report.provenance.engrams_with_sources
    );
    if let Some(ref newest) = report.recency.newest_updated {
        println!("newest_updated: {newest}");
    }
    if let Some(ref oldest) = report.recency.oldest_updated {
        println!("oldest_updated: {oldest}");
    }
    println!("density_neighbors: {}", report.density_neighbors);
    println!(
        "detail: {} claim tokens, {} body tokens (ratio {:.2}{})",
        report.claim_tokens,
        report.body_tokens,
        report.detail_ratio,
        if report.detail_sparse {
            ", sparse unless expanded"
        } else {
            ""
        }
    );
    println!("meta_reliability: {:.2}", report.meta_reliability);
    println!("recommended_next: {}", report.recommended_next.as_str());
}
