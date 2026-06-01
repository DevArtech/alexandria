use std::path::PathBuf;

use alexandria_core::{export_pack, Config, Index, Library, PackExportOptions};
use anyhow::Result;

use crate::OutputFormat;

pub fn run_export(
    library_path: Option<PathBuf>,
    format: OutputFormat,
    target: PathBuf,
    name: Option<String>,
    collections: Vec<String>,
    tags: Vec<String>,
    include_archived: bool,
) -> Result<()> {
    let library = match library_path {
        Some(p) => Library::discover(Some(&p))?,
        None => Library::discover(None)?,
    };
    let config = Config::load(&library.root)?;
    let index = Index::open_readonly(&library)?;

    let report = export_pack(
        &library,
        &index,
        &config,
        &PackExportOptions {
            target,
            name,
            collections,
            tags,
            include_archived,
            alexandria_version: env!("CARGO_PKG_VERSION").into(),
        },
    )?;

    match format {
        OutputFormat::Human => {
            println!("Exported pack to {}", report.path);
            println!("name: {}", report.name);
            println!("engrams: {}", report.engram_count);
            println!("skipped_relational: {}", report.skipped_relational);
            println!("skipped_archived: {}", report.skipped_archived);
            println!("skipped_no_match: {}", report.skipped_no_match);
        }
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&report)?),
    }
    Ok(())
}
