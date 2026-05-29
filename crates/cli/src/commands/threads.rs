use std::path::PathBuf;

use alexandria_core::{list_threads, Config, Index, Library};
use anyhow::Result;

use crate::OutputFormat;

pub fn run(
    library_path: Option<PathBuf>,
    format: OutputFormat,
    surface_for: Option<String>,
) -> Result<()> {
    let library = match library_path {
        Some(p) => Library::discover(Some(&p))?,
        None => Library::discover(None)?,
    };
    let config = Config::load(&library.root)?;
    let index = Index::open_readonly(&library)?;
    let _ = config;
    let result = list_threads(&library, &index, surface_for.as_deref())?;

    match format {
        OutputFormat::Human => {
            if let Some(topic) = &result.surface_for {
                println!("surface_for: {topic}");
            }
            if result.threads.is_empty() {
                println!("(no open threads)");
            }
            for t in &result.threads {
                println!(
                    "[{}] {} (last_touched: {}, dormant {:.1}d, triggers: {})",
                    t.id,
                    t.claim,
                    t.last_touched.format("%Y-%m-%d"),
                    t.dormant_days,
                    t.surface_when.join(", ")
                );
            }
        }
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&result)?),
    }
    Ok(())
}
