use std::path::PathBuf;

use alexandria_core::Library;
use anyhow::Result;

use crate::OutputFormat;

pub fn run(path: Option<PathBuf>, format: OutputFormat) -> Result<()> {
    let path = path.unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    let library = Library::init(&path)?;

    match format {
        OutputFormat::Human => {
            println!("Initialized Alexandria library at {}", library.root.display());
        }
        OutputFormat::Json => {
            println!(
                "{}",
                serde_json::json!({
                    "status": "initialized",
                    "path": library.root.display().to_string()
                })
            );
        }
    }
    Ok(())
}
