use std::path::PathBuf;

use alexandria_core::{
    build_graph_view, render_graph_view, Config, GraphRenderView, GraphScope, GraphViewOptions,
    Index, Library, Rel,
};
use anyhow::{bail, Result};

use crate::commands::util::parse_rel_cli;
use crate::OutputFormat;

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum GraphScopeArg {
    Global,
    Seed,
    Provenance,
}

impl From<GraphScopeArg> for GraphScope {
    fn from(v: GraphScopeArg) -> Self {
        match v {
            GraphScopeArg::Global => GraphScope::Global,
            GraphScopeArg::Seed => GraphScope::Seed,
            GraphScopeArg::Provenance => GraphScope::Provenance,
        }
    }
}

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum GraphViewArg {
    Ascii,
    Mermaid,
    Dot,
}

impl From<GraphViewArg> for GraphRenderView {
    fn from(v: GraphViewArg) -> Self {
        match v {
            GraphViewArg::Ascii => GraphRenderView::Ascii,
            GraphViewArg::Mermaid => GraphRenderView::Mermaid,
            GraphViewArg::Dot => GraphRenderView::Dot,
        }
    }
}

pub struct GraphOptions {
    pub library_path: Option<PathBuf>,
    pub format: OutputFormat,
    pub seed: Option<String>,
    pub scope: GraphScopeArg,
    pub view: GraphViewArg,
    pub depth: Option<u32>,
    pub rels: Vec<String>,
    pub max_nodes: usize,
    pub overlay_facets: bool,
    pub interactive: bool,
}

pub fn run(opts: GraphOptions) -> Result<()> {
    if opts.interactive {
        return super::graph_tui::run(
            opts.library_path,
            opts.seed,
            opts.scope,
            opts.depth,
            opts.rels,
            opts.max_nodes,
            opts.overlay_facets,
        );
    }

    let library = match opts.library_path {
        Some(p) => Library::discover(Some(&p))?,
        None => Library::discover(None)?,
    };
    let config = Config::load(&library.root)?;
    let index = Index::open(&library, &config)?;

    if matches!(opts.scope, GraphScopeArg::Seed | GraphScopeArg::Provenance) && opts.seed.is_none()
    {
        bail!("--seed is required for seed and provenance scopes");
    }

    let parsed_rels = if opts.rels.is_empty() {
        None
    } else {
        Some(
            opts.rels
                .iter()
                .map(|r| parse_rel_cli(r))
                .collect::<Result<Vec<Rel>, _>>()?,
        )
    };

    let view_result = build_graph_view(
        &index,
        &config,
        GraphViewOptions {
            scope: opts.scope.into(),
            seed: opts.seed,
            depth: opts.depth.unwrap_or(2),
            rels: parsed_rels,
            max_nodes: opts.max_nodes,
            overlay_facets: opts.overlay_facets,
        },
    )?;

    match opts.format {
        OutputFormat::Human => print_human(&view_result, opts.view),
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&view_result)?),
    }
    Ok(())
}

pub fn print_human(result: &alexandria_core::GraphView, view: GraphViewArg) {
    if result.truncated {
        println!("(graph truncated at {} nodes)", result.node_count);
    }
    println!("{}", render_graph_view(result, view.into()));
}
