use std::path::PathBuf;

use alexandria_core::{Config, Index, Library};
use anyhow::Result;

use crate::OutputFormat;

pub fn run(library_path: Option<PathBuf>, format: OutputFormat) -> Result<()> {
    let library = match library_path {
        Some(p) => Library::discover(Some(&p))?,
        None => Library::discover(None)?,
    };
    let config = Config::load(&library.root)?;
    let index = Index::open(&library, &config)?;
    let result = index.reindex(&library)?;

    match format {
        OutputFormat::Human => {
            println!("Reindexed {} engrams", result.indexed);
            for failure in &result.parse_failures {
                eprintln!(
                    "Warning: failed to parse {}: {}",
                    failure.path.display(),
                    failure.error
                );
            }
        }
        OutputFormat::Json => {
            println!(
                "{}",
                serde_json::json!({
                    "status": "reindexed",
                    "count": result.indexed,
                    "parse_failures": result.parse_failures.iter().map(|f| {
                        serde_json::json!({
                            "path": f.path.display().to_string(),
                            "error": f.error,
                        })
                    }).collect::<Vec<_>>(),
                })
            );
        }
    }
    Ok(())
}
