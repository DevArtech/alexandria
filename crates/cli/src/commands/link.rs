use std::path::PathBuf;

use alexandria_core::{Config, Index, Library, Ops};
use anyhow::Result;

use crate::OutputFormat;
use crate::commands::util::parse_rel_cli;

pub fn run(
    library_path: Option<PathBuf>,
    format: OutputFormat,
    from: String,
    rel: String,
    to: String,
) -> Result<()> {
    let library = discover(library_path)?;
    let config = Config::load(&library.root)?;
    let index = Index::open(&library, &config)?;
    let rel = parse_rel_cli(&rel)?;
    let ops = Ops::new(&library, &index);
    let result = ops.link(&from, rel, &to)?;

    match format {
        OutputFormat::Human => {
            println!("Linked {} --{}--> {}", result.from_id, result.rel, result.to_id);
            if result.reciprocal_added {
                println!("  reciprocal edge added");
            }
            if result.target_superseded {
                println!("  target marked superseded");
            }
        }
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&result)?),
    }
    Ok(())
}

fn discover(library_path: Option<PathBuf>) -> Result<Library> {
    match library_path {
        Some(p) => Ok(Library::discover(Some(&p))?),
        None => Ok(Library::discover(None)?),
    }
}
