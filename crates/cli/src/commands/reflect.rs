use std::path::PathBuf;

use alexandria_core::{Config, Index, Library, consolidate_fast, consolidate_slow};
use anyhow::Result;

use crate::OutputFormat;

pub fn run(
    library_path: Option<PathBuf>,
    format: OutputFormat,
    fast: bool,
) -> Result<()> {
    let library = match library_path {
        Some(p) => Library::discover(Some(&p))?,
        None => Library::discover(None)?,
    };
    let config = Config::load(&library.root)?;

    if fast {
        let report = consolidate_fast(&library, &config)?;
        match format {
            OutputFormat::Human => {
                println!("fast reflection complete");
                println!("briefing: {}", report.briefing_path);
                println!("engrams summarized: {}", report.engrams_summarized);
            }
            OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&report)?),
        }
        return Ok(());
    }

    let index = Index::open(&library, &config)?;
    let report = consolidate_slow(&library, &index, &config, None)?;

    match format {
        OutputFormat::Human => {
            println!("slow reflection complete");
            crate::commands::consolidate::print_human(&report);
        }
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&report)?),
    }
    Ok(())
}
