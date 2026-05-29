use std::path::PathBuf;

use alexandria_core::{Config, Index, Library, Ops};
use anyhow::Result;

use crate::OutputFormat;

pub fn run(library_path: Option<PathBuf>, format: OutputFormat, id: String) -> Result<()> {
    let library = match library_path {
        Some(p) => Library::discover(Some(&p))?,
        None => Library::discover(None)?,
    };
    let config = Config::load(&library.root)?;
    let index = Index::open(&library, &config)?;
    let ops = Ops::new(&library, &index);
    let result = ops.archive(&id)?;

    match format {
        OutputFormat::Human => {
            println!("Archived {} ({})", result.id, result.claim);
            println!("  path: {}", result.path);
        }
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&result)?),
    }
    Ok(())
}
