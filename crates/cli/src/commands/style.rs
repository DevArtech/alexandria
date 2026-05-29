use std::path::PathBuf;

use alexandria_core::{style_profile, Library};
use anyhow::Result;

use crate::OutputFormat;

pub fn run(library_path: Option<PathBuf>, format: OutputFormat, profile: bool) -> Result<()> {
    let library = match library_path {
        Some(p) => Library::discover(Some(&p))?,
        None => Library::discover(None)?,
    };
    let style = style_profile(&library, None)?;

    match format {
        OutputFormat::Human => {
            if profile {
                println!("verbosity: {:.2}", style.verbosity);
                println!("directness: {:.2}", style.directness);
                println!("hedging: {:.2}", style.hedging);
                println!("pushback_tolerance: {:.2}", style.pushback_tolerance);
                println!("pacing: {}", style.pacing);
                if let Some(ev) = &style.evidence_summary {
                    println!(
                        "evidence: projects={} task_types={} registers={}",
                        ev.projects, ev.task_types, ev.registers
                    );
                }
            } else {
                println!(
                    "verbosity={:.2} directness={:.2} hedging={:.2} pushback={:.2} pacing={}",
                    style.verbosity,
                    style.directness,
                    style.hedging,
                    style.pushback_tolerance,
                    style.pacing
                );
            }
        }
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&style)?),
    }
    Ok(())
}
