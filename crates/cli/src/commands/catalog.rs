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

    format_catalog(&cat, format)
}

pub fn format_catalog(cat: &alexandria_core::Catalog, format: OutputFormat) -> Result<()> {
    match format {
        OutputFormat::Human => {
            print_human(cat);
            Ok(())
        }
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(cat)?);
            Ok(())
        }
    }
}

pub fn print_human(cat: &alexandria_core::Catalog) {
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
