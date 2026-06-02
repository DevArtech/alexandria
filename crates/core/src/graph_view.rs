//! Build unified graph views for visualization (seeded, global, provenance scopes).

use std::collections::{BTreeSet, HashMap, HashSet};

use rusqlite::params;
use serde::{Deserialize, Serialize};

use crate::engram::{Rel, Status, Tier};
use crate::error::{AlexandriaError, Result};
use crate::graph::Graph;
use crate::index::Index;
use crate::map::{map, MapOptions};
use crate::render::{from_trace_result, RenderEdge, RenderNode};
use crate::Config;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GraphScope {
    Global,
    Seed,
    Provenance,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphViewNode {
    pub id: String,
    pub claim: String,
    pub tier: String,
    pub status: String,
    pub confidence: f64,
    pub collections: Vec<String>,
    pub tags: Vec<String>,
    pub is_seed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphViewEdge {
    pub from_id: String,
    pub to_id: String,
    pub rel: String,
    pub depth: u32,
    pub kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphView {
    pub scope: String,
    pub seed: Option<String>,
    pub depth: u32,
    pub nodes: Vec<GraphViewNode>,
    pub edges: Vec<GraphViewEdge>,
    pub node_count: usize,
    pub edge_count: usize,
    pub truncated: bool,
}

pub struct GraphViewOptions {
    pub scope: GraphScope,
    pub seed: Option<String>,
    pub depth: u32,
    pub rels: Option<Vec<Rel>>,
    pub max_nodes: usize,
    pub overlay_facets: bool,
}

impl Default for GraphViewOptions {
    fn default() -> Self {
        Self {
            scope: GraphScope::Seed,
            seed: None,
            depth: 2,
            rels: None,
            max_nodes: 200,
            overlay_facets: false,
        }
    }
}

/// Build a graph view for visualization/export.
pub fn build_graph_view(
    index: &Index,
    config: &Config,
    options: GraphViewOptions,
) -> Result<GraphView> {
    let depth = options.depth.clamp(1, 10);
    match options.scope {
        GraphScope::Global => build_global_view(index, options.max_nodes, options.overlay_facets),
        GraphScope::Seed => {
            let seed = options.seed.clone().ok_or_else(|| {
                AlexandriaError::Other(anyhow::anyhow!("seed required for seed scope"))
            })?;
            build_seed_view(
                index,
                config,
                &seed,
                depth,
                options.rels,
                options.max_nodes,
                options.overlay_facets,
            )
        }
        GraphScope::Provenance => {
            let seed = options.seed.clone().ok_or_else(|| {
                AlexandriaError::Other(anyhow::anyhow!("seed required for provenance scope"))
            })?;
            build_provenance_view(index, &seed, options.max_nodes, options.overlay_facets)
        }
    }
}

fn build_global_view(index: &Index, max_nodes: usize, overlay_facets: bool) -> Result<GraphView> {
    let conn = index.connection();
    let mut stmt = conn.prepare(
        r#"
        SELECT e.from_id, ef.claim, ef.tier, ef.status, ef.confidence,
               e.rel, e.to_id, et.claim, et.tier, et.status, et.confidence
        FROM edges e
        JOIN engrams ef ON ef.id = e.from_id
        JOIN engrams et ON et.id = e.to_id
        WHERE ef.tier != 'relational' AND et.tier != 'relational'
        ORDER BY e.rel, e.from_id, e.to_id
        "#,
    )?;

    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, f64>(4)?,
            row.get::<_, String>(5)?,
            row.get::<_, String>(6)?,
            row.get::<_, String>(7)?,
            row.get::<_, String>(8)?,
            row.get::<_, String>(9)?,
            row.get::<_, f64>(10)?,
        ))
    })?;

    let mut nodes: HashMap<String, GraphViewNode> = HashMap::new();
    let mut edges = Vec::new();
    let mut truncated = false;

    for row in rows {
        let (
            from_id,
            from_claim,
            from_tier,
            from_status,
            from_conf,
            rel,
            to_id,
            to_claim,
            to_tier,
            to_status,
            to_conf,
        ) = row?;

        if nodes.len() >= max_nodes && !nodes.contains_key(&from_id) && !nodes.contains_key(&to_id)
        {
            truncated = true;
            continue;
        }

        nodes
            .entry(from_id.clone())
            .or_insert_with(|| GraphViewNode {
                id: from_id.clone(),
                claim: from_claim,
                tier: from_tier,
                status: from_status,
                confidence: from_conf,
                collections: Vec::new(),
                tags: Vec::new(),
                is_seed: false,
            });
        nodes.entry(to_id.clone()).or_insert_with(|| GraphViewNode {
            id: to_id.clone(),
            claim: to_claim,
            tier: to_tier,
            status: to_status,
            confidence: to_conf,
            collections: Vec::new(),
            tags: Vec::new(),
            is_seed: false,
        });

        if nodes.len() <= max_nodes {
            edges.push(GraphViewEdge {
                from_id,
                to_id,
                rel,
                depth: 1,
                kind: "typed".to_string(),
            });
        }
    }

    if overlay_facets {
        enrich_facets(index, &mut nodes)?;
    }

    let node_count = nodes.len();
    let edge_count = edges.len();
    Ok(GraphView {
        scope: "global".to_string(),
        seed: None,
        depth: 1,
        nodes: nodes.into_values().collect(),
        edges,
        node_count,
        edge_count,
        truncated,
    })
}

fn build_seed_view(
    index: &Index,
    config: &Config,
    seed: &str,
    depth: u32,
    rels: Option<Vec<Rel>>,
    max_nodes: usize,
    overlay_facets: bool,
) -> Result<GraphView> {
    // Resolve seeds via map helper (recall/fts fallback) without budget trimming.
    let map_result = map(
        seed,
        index,
        config,
        MapOptions {
            depth,
            rels: rels.clone(),
            budget: Some(u32::MAX),
        },
    )?;

    let seed_ids = map_result.seed_ids.clone();
    let seed_set: HashSet<_> = seed_ids.iter().cloned().collect();
    let mut nodes: HashMap<String, GraphViewNode> = HashMap::new();
    let mut edges = Vec::new();
    let mut truncated = false;

    for group in &map_result.rel_groups {
        for edge in &group.edges {
            if nodes.len() >= max_nodes {
                truncated = true;
                break;
            }
            upsert_seed_node(
                &mut nodes,
                &edge.from_id,
                &edge.from_claim,
                &seed_set,
                index,
            )?;
            upsert_seed_node(&mut nodes, &edge.to_id, &edge.to_claim, &seed_set, index)?;
            edges.push(GraphViewEdge {
                from_id: edge.from_id.clone(),
                to_id: edge.to_id.clone(),
                rel: edge.rel.clone(),
                depth: edge.depth,
                kind: "typed".to_string(),
            });
        }
    }

    for id in &seed_ids {
        if nodes.len() >= max_nodes {
            truncated = true;
            break;
        }
        if !nodes.contains_key(id) {
            if let Some(row) = index.get_engram(id)? {
                nodes.insert(
                    id.clone(),
                    GraphViewNode {
                        id: id.clone(),
                        claim: row.claim,
                        tier: tier_str(row.tier).to_string(),
                        status: status_str(row.status).to_string(),
                        confidence: row.confidence,
                        collections: row.collections,
                        tags: Vec::new(),
                        is_seed: true,
                    },
                );
            }
        }
    }

    if overlay_facets {
        enrich_facets(index, &mut nodes)?;
    }

    let node_count = nodes.len();
    let edge_count = edges.len();
    Ok(GraphView {
        scope: "seed".to_string(),
        seed: Some(seed.to_string()),
        depth,
        nodes: nodes.into_values().collect(),
        edges,
        node_count,
        edge_count,
        truncated,
    })
}

fn upsert_seed_node(
    nodes: &mut HashMap<String, GraphViewNode>,
    id: &str,
    claim: &str,
    seeds: &HashSet<String>,
    index: &Index,
) -> Result<()> {
    if nodes.contains_key(id) {
        if let Some(n) = nodes.get_mut(id) {
            n.is_seed = n.is_seed || seeds.contains(id);
        }
        return Ok(());
    }
    if let Some(row) = index.get_engram(id)? {
        nodes.insert(
            id.to_string(),
            GraphViewNode {
                id: id.to_string(),
                claim: row.claim,
                tier: tier_str(row.tier).to_string(),
                status: status_str(row.status).to_string(),
                confidence: row.confidence,
                collections: row.collections,
                tags: Vec::new(),
                is_seed: seeds.contains(id),
            },
        );
    } else {
        nodes.insert(
            id.to_string(),
            GraphViewNode {
                id: id.to_string(),
                claim: claim.to_string(),
                tier: String::new(),
                status: String::new(),
                confidence: 0.0,
                collections: Vec::new(),
                tags: Vec::new(),
                is_seed: seeds.contains(id),
            },
        );
    }
    Ok(())
}

fn build_provenance_view(
    index: &Index,
    seed: &str,
    max_nodes: usize,
    overlay_facets: bool,
) -> Result<GraphView> {
    let graph = Graph::new(index);
    let trace = graph.trace(seed)?;
    let (render_nodes, render_edges) = from_trace_result(&trace);

    let truncated = render_nodes.len() > max_nodes;
    let nodes: Vec<GraphViewNode> = render_nodes
        .into_iter()
        .take(max_nodes)
        .map(|n| GraphViewNode {
            id: n.id,
            claim: n.claim,
            tier: n.tier,
            status: n.status,
            confidence: 0.0,
            collections: n.collections,
            tags: n.tags,
            is_seed: n.is_seed,
        })
        .collect();

    let node_ids: BTreeSet<_> = nodes.iter().map(|n| n.id.clone()).collect();
    let edges: Vec<GraphViewEdge> = render_edges
        .into_iter()
        .filter(|e| node_ids.contains(&e.from_id) && node_ids.contains(&e.to_id))
        .map(|e| GraphViewEdge {
            from_id: e.from_id,
            to_id: e.to_id,
            rel: e.rel,
            depth: e.depth,
            kind: e.kind,
        })
        .collect();

    let mut nodes_map: HashMap<String, GraphViewNode> =
        nodes.into_iter().map(|n| (n.id.clone(), n)).collect();

    // Fill confidence from index
    for node in nodes_map.values_mut() {
        if let Ok(Some(row)) = index.get_engram(&node.id) {
            node.confidence = row.confidence;
            if node.tier.is_empty() {
                node.tier = tier_str(row.tier).to_string();
            }
            if node.status.is_empty() {
                node.status = status_str(row.status).to_string();
            }
        }
    }

    if overlay_facets {
        enrich_facets(index, &mut nodes_map)?;
    }

    let node_count = nodes_map.len();
    let edge_count = edges.len();
    Ok(GraphView {
        scope: "provenance".to_string(),
        seed: Some(seed.to_string()),
        depth: trace.nodes.iter().map(|n| n.depth).max().unwrap_or(0),
        nodes: nodes_map.into_values().collect(),
        edges,
        node_count,
        edge_count,
        truncated,
    })
}

fn enrich_facets(index: &Index, nodes: &mut HashMap<String, GraphViewNode>) -> Result<()> {
    let conn = index.connection();
    for node in nodes.values_mut() {
        let mut col_stmt = conn.prepare(
            "SELECT collection FROM collection_members WHERE engram_id = ?1 ORDER BY collection",
        )?;
        node.collections = col_stmt
            .query_map(params![&node.id], |row| row.get(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        let mut tag_stmt =
            conn.prepare("SELECT tag FROM tags WHERE engram_id = ?1 ORDER BY tag")?;
        node.tags = tag_stmt
            .query_map(params![&node.id], |row| row.get(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
    }
    Ok(())
}

/// Convert a `GraphView` into render-layer nodes/edges.
pub fn graph_view_to_render(view: &GraphView) -> (Vec<RenderNode>, Vec<RenderEdge>) {
    let nodes = view
        .nodes
        .iter()
        .map(|n| RenderNode {
            id: n.id.clone(),
            claim: n.claim.clone(),
            tier: n.tier.clone(),
            status: n.status.clone(),
            collections: n.collections.clone(),
            tags: n.tags.clone(),
            is_seed: n.is_seed,
        })
        .collect();
    let edges = view
        .edges
        .iter()
        .map(|e| RenderEdge {
            from_id: e.from_id.clone(),
            to_id: e.to_id.clone(),
            rel: e.rel.clone(),
            depth: e.depth,
            kind: e.kind.clone(),
        })
        .collect();
    (nodes, edges)
}

/// Render a graph view using the selected view format (ASCII/Mermaid/DOT string).
fn tier_str(tier: Tier) -> &'static str {
    match tier {
        Tier::Working => "working",
        Tier::Episodic => "episodic",
        Tier::Provisional => "provisional",
        Tier::Semantic => "semantic",
        Tier::Procedural => "procedural",
        Tier::Relational => "relational",
    }
}

fn status_str(status: Status) -> &'static str {
    match status {
        Status::Confirmed => "confirmed",
        Status::Provisional => "provisional",
        Status::UnresolvedByDesign => "unresolved_by_design",
        Status::Superseded => "superseded",
        Status::Archived => "archived",
    }
}

pub fn render_graph_view(view: &GraphView, render_view: crate::render::GraphRenderView) -> String {
    use crate::render::{render_ascii_graph, render_dot, render_mermaid};
    let (nodes, edges) = graph_view_to_render(view);
    match render_view {
        crate::render::GraphRenderView::Ascii => {
            if view.scope == "provenance" {
                // Prefer tree-style provenance when scope is provenance-only nodes
                render_ascii_graph(&view.scope, view.seed.as_deref(), &nodes, &edges)
            } else {
                render_ascii_graph(&view.scope, view.seed.as_deref(), &nodes, &edges)
            }
        }
        crate::render::GraphRenderView::Mermaid => render_mermaid(&nodes, &edges),
        crate::render::GraphRenderView::Dot => render_dot(&nodes, &edges),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engram::{Engram, Link, Status, Tier};
    use crate::provider::build_embedder;
    use crate::{Index, Library};

    fn setup() -> (tempfile::TempDir, Library, Index, Config) {
        let dir = tempfile::TempDir::new().unwrap();
        let lib = Library::init(dir.path()).unwrap();
        let mut config = Config::load(dir.path()).unwrap();
        config.providers.embedder = "hash".into();
        let embedder = build_embedder(&config).unwrap();
        let index = Index::open_with_embedder(&lib, embedder).unwrap();
        (dir, lib, index, config)
    }

    fn remember(lib: &Library, index: &Index, engram: &Engram) {
        let p = lib.write_engram(engram).unwrap();
        index.upsert(engram, &p.display().to_string()).unwrap();
    }

    #[test]
    fn build_seed_view_from_linked_engrams() {
        let (_dir, lib, index, config) = setup();
        let mut a = Engram::new("alpha", "b", Tier::Semantic, Status::Confirmed);
        let b = Engram::new("beta", "b", Tier::Semantic, Status::Confirmed);
        a.links.push(Link {
            rel: Rel::Supports,
            to: b.id.clone(),
        });
        remember(&lib, &index, &b);
        remember(&lib, &index, &a);

        let view = build_graph_view(
            &index,
            &config,
            GraphViewOptions {
                scope: GraphScope::Seed,
                seed: Some(a.id.clone()),
                depth: 2,
                rels: None,
                max_nodes: 50,
                overlay_facets: false,
            },
        )
        .unwrap();

        assert_eq!(view.scope, "seed");
        assert!(!view.edges.is_empty());
        assert!(view.nodes.iter().any(|n| n.id == a.id));
    }
}
