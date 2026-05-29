use std::path::PathBuf;

use alexandria_core::{Graph, Index, Library};
use anyhow::Result;

use crate::OutputFormat;

pub fn run(library_path: Option<PathBuf>, format: OutputFormat, id: String) -> Result<()> {
    let library = match library_path {
        Some(p) => Library::discover(Some(&p))?,
        None => Library::discover(None)?,
    };
    let index = Index::open_readonly(&library)?;
    let graph = Graph::new(&index);
    let result = graph.trace(&id)?;

    match format {
        OutputFormat::Human => print_human(&result),
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&result)?),
    }
    Ok(())
}

fn print_human(result: &alexandria_core::TraceResult) {
    println!("[{}] {}", result.id, result.claim);
    println!(
        "confidence: {:.2} (effective: {:.2})",
        result.confidence, result.effective_confidence
    );
    println!("derived_sources: {}", result.has_derived_sources);
    if result.nodes.is_empty() {
        println!("(no provenance nodes)");
        return;
    }
    for node in &result.nodes {
        println!(
            "  depth {}: [{}] {} via {} -> {}",
            node.depth, node.id, node.claim, node.source_kind, node.source_ref
        );
    }
}
