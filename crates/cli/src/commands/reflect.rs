use std::path::PathBuf;

use alexandria_core::{Config, Index, Library, consolidate_slow};
use anyhow::{bail, Result};

use crate::OutputFormat;

pub fn run(
    library_path: Option<PathBuf>,
    format: OutputFormat,
    fast: bool,
) -> Result<()> {
    if fast {
        bail!("reflect --fast is not implemented until M4");
    }

    let library = match library_path {
        Some(p) => Library::discover(Some(&p))?,
        None => Library::discover(None)?,
    };
    let config = Config::load(&library.root)?;
    let index = Index::open(&library, &config)?;
    let report = consolidate_slow(&library, &index, &config)?;

    match format {
        OutputFormat::Human => {
            println!("slow reflection complete");
            crate::commands::consolidate::print_human(&report);
        }
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&report)?),
    }
    Ok(())
}
