use std::path::PathBuf;

use alexandria_core::{
    from_map_result, map, render_dot, render_map_ascii, Config, GraphRenderView, Index, Library,
    MapOptions, Rel,
};
use anyhow::Result;

use crate::commands::util::parse_rel_cli;
use crate::OutputFormat;

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum MapView {
    Ascii,
    Mermaid,
    Dot,
}

impl From<MapView> for GraphRenderView {
    fn from(v: MapView) -> Self {
        match v {
            MapView::Ascii => GraphRenderView::Ascii,
            MapView::Mermaid => GraphRenderView::Mermaid,
            MapView::Dot => GraphRenderView::Dot,
        }
    }
}

pub fn run(
    library_path: Option<PathBuf>,
    format: OutputFormat,
    seed: String,
    depth: Option<u32>,
    rels: Vec<String>,
    budget: Option<u32>,
    view: MapView,
) -> Result<()> {
    let library = match library_path {
        Some(p) => Library::discover(Some(&p))?,
        None => Library::discover(None)?,
    };
    let config = Config::load(&library.root)?;
    let index = Index::open(&library, &config)?;

    let parsed_rels = if rels.is_empty() {
        None
    } else {
        Some(
            rels.iter()
                .map(|r| parse_rel_cli(r))
                .collect::<Result<Vec<Rel>, _>>()?,
        )
    };

    let result = map(
        &seed,
        &index,
        &config,
        MapOptions {
            depth: depth.unwrap_or(2),
            rels: parsed_rels,
            budget,
        },
    )?;

    match format {
        OutputFormat::Human => print_human(&result, view),
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&result)?),
    }
    Ok(())
}

pub fn print_human(result: &alexandria_core::MapResult, view: MapView) {
    match view {
        MapView::Ascii => {
            println!("{}", render_map_ascii(result));
        }
        MapView::Mermaid => {
            print_map_summary(result);
            if result.mermaid.is_empty() {
                println!("\n(no diagram)");
            } else {
                println!("\n{}", result.mermaid);
            }
        }
        MapView::Dot => {
            print_map_summary(result);
            let (nodes, edges) = from_map_result(result, &result.seed_ids);
            println!("\n{}", render_dot(&nodes, &edges));
        }
    }
}

fn print_map_summary(result: &alexandria_core::MapResult) {
    println!("seed: {}", result.seed);
    println!("depth: {}", result.depth);
    println!("edge_count: {}", result.edge_count);
    println!("total_tokens: {}", result.total_tokens);
}
