use std::path::PathBuf;

use alexandria_core::{Config, Index, Library, consolidate_slow};
use anyhow::Result;

use crate::OutputFormat;

pub fn run(library_path: Option<PathBuf>, format: OutputFormat) -> Result<()> {
    let library = match library_path {
        Some(p) => Library::discover(Some(&p))?,
        None => Library::discover(None)?,
    };
    let config = Config::load(&library.root)?;
    let index = Index::open(&library, &config)?;
    let report = consolidate_slow(&library, &index, &config, None)?;

    match format {
        OutputFormat::Human => print_human(&report),
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&report)?),
    }
    Ok(())
}

pub fn print_human(report: &alexandria_core::ConsolidationReport) {
    println!("merged: {}", report.merged.len());
    for item in &report.merged {
        println!("  {item}");
    }
    println!("promoted: {}", report.promoted.len());
    for item in &report.promoted {
        println!("  {item}");
    }
    println!("demoted: {}", report.demoted.len());
    for item in &report.demoted {
        println!("  {item}");
    }
    println!("decayed: {}", report.decayed.len());
    println!("collections_resummarized: {}", report.collections_resummarized.len());
    for item in &report.collections_resummarized {
        println!("  {item}");
    }
    println!("shapes_extracted: {}", report.shapes_extracted.len());
    println!("relational_decayed: {}", report.relational_decayed.len());
}
