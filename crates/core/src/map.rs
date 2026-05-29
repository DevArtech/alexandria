//! Relationship / concept graph surfacing via typed-edge traversal.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use serde::Serialize;

use crate::config::Config;
use crate::engram::Rel;
use crate::error::{AlexandriaError, Result};
use crate::graph::Graph;
use crate::index::Index;
use crate::retrieval::{RecallOptions, Retrieval};

#[derive(Debug, Clone, Serialize)]
pub struct MapEdge {
    pub from_id: String,
    pub from_claim: String,
    pub rel: String,
    pub to_id: String,
    pub to_claim: String,
    pub depth: u32,
    pub token_cost: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct MapRelGroup {
    pub rel: String,
    pub edges: Vec<MapEdge>,
    pub token_cost: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct MapResult {
    pub seed: String,
    pub depth: u32,
    pub rel_groups: Vec<MapRelGroup>,
    pub mermaid: String,
    pub total_tokens: u32,
    pub edge_count: u32,
}

pub struct MapOptions {
    pub depth: u32,
    pub rels: Option<Vec<Rel>>,
    pub budget: Option<u32>,
}

impl Default for MapOptions {
    fn default() -> Self {
        Self {
            depth: 2,
            rels: None,
            budget: None,
        }
    }
}

/// Build a concept graph from an engram id or topic string.
pub fn map(
    seed: &str,
    index: &Index,
    config: &Config,
    options: MapOptions,
) -> Result<MapResult> {
    let budget = options.budget.unwrap_or(config.budgets.default_recall_tokens);
    let depth = options.depth.clamp(1, 10);
    let rels_ref = options.rels.as_deref();

    let seed_ids = resolve_seeds(seed, index, config)?;
    if seed_ids.is_empty() {
        return Err(AlexandriaError::EngramNotFound(seed.to_string()));
    }

    let graph = Graph::new(index);
    let mut edges: Vec<MapEdge> = Vec::new();
    let mut seen_edges: BTreeSet<(String, String, String)> = BTreeSet::new();

    for from_id in &seed_ids {
        let trav = graph.traverse(from_id, rels_ref, depth)?;
        for node in trav.nodes {
            if node.tier == "relational" {
                continue;
            }
            let key = (from_id.clone(), node.rel.clone(), node.id.clone());
            if !seen_edges.insert(key) {
                continue;
            }
            let from_claim = index
                .get_engram(from_id)?
                .map(|r| r.claim)
                .unwrap_or_else(|| from_id.clone());
            let token_cost = crate::engram::Engram::estimate_tokens(&node.claim) + 4;
            edges.push(MapEdge {
                from_id: from_id.clone(),
                from_claim,
                rel: node.rel,
                to_id: node.id,
                to_claim: node.claim,
                depth: node.depth,
                token_cost,
            });
        }
    }

    edges.sort_by(|a, b| {
        a.rel
            .cmp(&b.rel)
            .then_with(|| a.depth.cmp(&b.depth))
            .then_with(|| a.to_id.cmp(&b.to_id))
    });

    let (rel_groups, total_tokens) = budget_trim_edges(edges, budget);
    let edge_count = rel_groups.iter().map(|g| g.edges.len() as u32).sum();
    let mermaid = render_mermaid(&seed_ids, &rel_groups);

    Ok(MapResult {
        seed: seed.to_string(),
        depth,
        rel_groups,
        mermaid,
        total_tokens,
        edge_count,
    })
}

fn resolve_seeds(seed: &str, index: &Index, config: &Config) -> Result<Vec<String>> {
    if index.get_engram(seed)?.is_some() {
        return Ok(vec![seed.to_string()]);
    }
    let retrieval = Retrieval::new(index, config);
    let recall = retrieval.recall(seed, Some(500), RecallOptions::default())?;
    let mut ids: Vec<String> = recall
        .tree
        .collections
        .iter()
        .flat_map(|c| c.hits.iter().take(3).map(|h| h.id.clone()))
        .collect();
    ids.sort();
    ids.dedup();
    if ids.is_empty() {
        let fts = index.fts_engram_ids(
            &crate::retrieval::escape_fts_query(seed),
            3,
        )?;
        return Ok(fts);
    }
    Ok(ids)
}

fn budget_trim_edges(edges: Vec<MapEdge>, budget: u32) -> (Vec<MapRelGroup>, u32) {
    let mut by_rel: BTreeMap<String, Vec<MapEdge>> = BTreeMap::new();
    for edge in edges {
        by_rel.entry(edge.rel.clone()).or_default().push(edge);
    }

    let mut groups = Vec::new();
    let mut total = 0u32;

    for (rel, mut rel_edges) in by_rel {
        let header_cost = crate::engram::Engram::estimate_tokens(&rel) + 2;
        if total + header_cost > budget && !groups.is_empty() {
            break;
        }
        let mut group_cost = header_cost;
        let mut kept = Vec::new();
        for edge in rel_edges.drain(..) {
            if total + group_cost + edge.token_cost > budget && !kept.is_empty() {
                break;
            }
            group_cost += edge.token_cost;
            kept.push(edge);
        }
        if kept.is_empty() && total + header_cost > budget {
            break;
        }
        total += group_cost;
        groups.push(MapRelGroup {
            rel: rel.clone(),
            token_cost: group_cost,
            edges: kept,
        });
    }

    (groups, total)
}

fn render_mermaid(seed_ids: &[String], groups: &[MapRelGroup]) -> String {
    let mut lines = vec!["flowchart LR".to_string()];
    let mut nodes: HashSet<String> = seed_ids.iter().cloned().collect();

    for group in groups {
        for edge in &group.edges {
            nodes.insert(edge.from_id.clone());
            nodes.insert(edge.to_id.clone());
        }
    }

    for id in &nodes {
        let label = id.replace('_', " ");
        lines.push(format!("  {id}[\"{label}\"]"));
    }

    for group in groups {
        for edge in &group.edges {
            let rel = edge.rel.replace('_', " ");
            lines.push(format!(
                "  {} -->|{}| {}",
                edge.from_id, rel, edge.to_id
            ));
        }
    }

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mermaid_includes_flowchart_header() {
        let groups = vec![MapRelGroup {
            rel: "supports".to_string(),
            token_cost: 10,
            edges: vec![MapEdge {
                from_id: "eng_a".to_string(),
                from_claim: "A".to_string(),
                rel: "supports".to_string(),
                to_id: "eng_b".to_string(),
                to_claim: "B".to_string(),
                depth: 1,
                token_cost: 5,
            }],
        }];
        let m = render_mermaid(&["eng_a".to_string()], &groups);
        assert!(m.contains("flowchart LR"));
        assert!(m.contains("supports"));
    }
}
