use std::path::PathBuf;

use alexandria_core::{catalog, regenerate_library_overview, Config, Index, Library};
use anyhow::Result;

use crate::OutputFormat;

pub fn run(
    library_path: Option<PathBuf>,
    format: OutputFormat,
    write_overview: bool,
) -> Result<()> {
    let library = match library_path {
        Some(p) => Library::discover(Some(&p))?,
        None => Library::discover(None)?,
    };
    let config = Config::load(&library.root)?;
    let index = Index::open_readonly(&library)?;
    let _ = config;
    let cat = catalog(&index)?;

    if write_overview {
        let overview = regenerate_library_overview(&library, &index)?;
        match format {
            OutputFormat::Human => {
                if overview.written {
                    println!("Wrote library overview to {}", overview.path);
                } else {
                    println!("Library overview unchanged at {}", overview.path);
                }
            }
            OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&overview)?),
        }
        return Ok(());
    }

    match format {
        OutputFormat::Human => {
            println!("total_engrams: {}", cat.total_engrams);
            println!("collections ({}):", cat.collections.len());
            for c in &cat.collections {
                println!("  {} ({})", c.name, c.count);
            }
            println!("tags ({}):", cat.tags.len());
            for t in &cat.tags {
                println!("  {} ({})", t.name, t.count);
            }
        }
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&cat)?),
    }
    Ok(())
}
