//! Dependency-free graph renderers (ASCII, DOT, Mermaid) for CLI and MCP output.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use serde::{Deserialize, Serialize};

use crate::graph::{TraceNode, TraceResult};
use crate::map::{MapRelGroup, MapResult};

/// Default claim truncation length for diagram labels.
pub const CLAIM_TRUNCATE: usize = 48;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GraphRenderView {
    Ascii,
    Mermaid,
    Dot,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenderNode {
    pub id: String,
    pub claim: String,
    pub tier: String,
    pub status: String,
    pub collections: Vec<String>,
    pub tags: Vec<String>,
    pub is_seed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenderEdge {
    pub from_id: String,
    pub to_id: String,
    pub rel: String,
    pub depth: u32,
    /// `typed` or `provenance`
    pub kind: String,
}

/// Truncate a claim for display, appending ellipsis when shortened.
pub fn truncate_claim(claim: &str, max: usize) -> String {
    let trimmed = claim.trim();
    if trimmed.chars().count() <= max {
        return trimmed.to_string();
    }
    let mut out: String = trimmed.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

fn escape_dot_label(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

fn escape_mermaid_label(s: &str) -> String {
    s.replace('"', "'").replace('\n', " ")
}

fn dot_node_id(id: &str) -> String {
    id.replace('-', "_")
}

fn mermaid_node_id(id: &str) -> String {
    id.replace('-', "_")
}

fn tier_dot_color(tier: &str) -> &'static str {
    match tier {
        "working" => "#9ca3af",
        "episodic" => "#3b82f6",
        "provisional" => "#eab308",
        "semantic" => "#22c55e",
        "procedural" => "#a855f7",
        _ => "#64748b",
    }
}

fn tier_mermaid_class(tier: &str) -> &'static str {
    match tier {
        "working" => "working",
        "episodic" => "episodic",
        "provisional" => "provisional",
        "semantic" => "semantic",
        "procedural" => "procedural",
        _ => "other",
    }
}

fn dot_edge_style(rel: &str) -> (&'static str, &'static str) {
    match rel {
        "conflicts_confirmed" | "tension_possible" => ("dashed", "red"),
        "supersedes" | "superseded_by" => ("dotted", "gray"),
        "derived_from" => ("solid", "blue"),
        _ => ("solid", "black"),
    }
}

fn status_dot_style(status: &str) -> Option<&'static str> {
    match status {
        "superseded" | "archived" => Some("dashed"),
        "unresolved_by_design" => Some("bold"),
        _ => None,
    }
}

/// Build render nodes/edges from a `MapResult` and resolved seed ids.
pub fn from_map_result(
    result: &MapResult,
    seed_ids: &[String],
) -> (Vec<RenderNode>, Vec<RenderEdge>) {
    let seed_set: BTreeSet<_> = seed_ids.iter().cloned().collect();
    let mut nodes: HashMap<String, RenderNode> = HashMap::new();
    let mut edges = Vec::new();

    for group in &result.rel_groups {
        for edge in &group.edges {
            nodes
                .entry(edge.from_id.clone())
                .or_insert_with(|| RenderNode {
                    id: edge.from_id.clone(),
                    claim: edge.from_claim.clone(),
                    tier: String::new(),
                    status: String::new(),
                    collections: Vec::new(),
                    tags: Vec::new(),
                    is_seed: seed_set.contains(&edge.from_id),
                });
            nodes
                .entry(edge.to_id.clone())
                .or_insert_with(|| RenderNode {
                    id: edge.to_id.clone(),
                    claim: edge.to_claim.clone(),
                    tier: String::new(),
                    status: String::new(),
                    collections: Vec::new(),
                    tags: Vec::new(),
                    is_seed: seed_set.contains(&edge.to_id),
                });
            edges.push(RenderEdge {
                from_id: edge.from_id.clone(),
                to_id: edge.to_id.clone(),
                rel: edge.rel.clone(),
                depth: edge.depth,
                kind: "typed".to_string(),
            });
        }
    }

    for seed in seed_ids {
        nodes.entry(seed.clone()).or_insert_with(|| RenderNode {
            id: seed.clone(),
            claim: seed.clone(),
            tier: String::new(),
            status: String::new(),
            collections: Vec::new(),
            tags: Vec::new(),
            is_seed: true,
        });
    }

    (nodes.into_values().collect(), edges)
}

/// Build render nodes/edges from a provenance trace.
pub fn from_trace_result(result: &TraceResult) -> (Vec<RenderNode>, Vec<RenderEdge>) {
    let mut nodes: HashMap<String, RenderNode> = HashMap::new();
    let mut edges = Vec::new();

    nodes.insert(
        result.id.clone(),
        RenderNode {
            id: result.id.clone(),
            claim: result.claim.clone(),
            tier: String::new(),
            status: String::new(),
            collections: Vec::new(),
            tags: Vec::new(),
            is_seed: true,
        },
    );

    for node in &result.nodes {
        insert_trace_node(&mut nodes, node);
        if node.source_kind == "derived" && !node.source_ref.is_empty() {
            edges.push(RenderEdge {
                from_id: node.id.clone(),
                to_id: node.source_ref.clone(),
                rel: "derived_from".to_string(),
                depth: node.depth,
                kind: "provenance".to_string(),
            });
        }
    }

    (nodes.into_values().collect(), edges)
}

fn insert_trace_node(nodes: &mut HashMap<String, RenderNode>, node: &TraceNode) {
    nodes
        .entry(node.id.clone())
        .and_modify(|n| {
            n.claim = node.claim.clone();
            n.tier = node.tier.clone();
            n.status = node.status.clone();
        })
        .or_insert_with(|| RenderNode {
            id: node.id.clone(),
            claim: node.claim.clone(),
            tier: node.tier.clone(),
            status: node.status.clone(),
            collections: Vec::new(),
            tags: Vec::new(),
            is_seed: node.depth == 0,
        });
}

/// Render a map result as an ASCII tree grouped by relationship.
pub fn render_map_ascii(result: &MapResult) -> String {
    let mut out = String::new();
    out.push_str(&format!("seed: {}\n", result.seed));
    out.push_str(&format!(
        "depth: {} | edges: {} | ~{} tokens\n",
        result.depth, result.edge_count, result.total_tokens
    ));

    if result.rel_groups.is_empty() {
        out.push_str("\n(no edges)\n");
        return out;
    }

    for group in &result.rel_groups {
        render_rel_group_ascii(&mut out, group);
    }

    out.push_str("\nlegend: [id] claim --rel--> [id] claim\n");
    out
}

fn render_rel_group_ascii(out: &mut String, group: &MapRelGroup) {
    out.push('\n');
    out.push_str(&format!(
        "## {} ({} edges, ~{} tokens)\n",
        group.rel,
        group.edges.len(),
        group.token_cost
    ));
    let last = group.edges.len().saturating_sub(1);
    for (i, edge) in group.edges.iter().enumerate() {
        let branch = if i == last { "└─" } else { "├─" };
        out.push_str(&format!(
            "{branch} [{}] {} --{}--> [{}] {} (depth {})\n",
            edge.from_id,
            truncate_claim(&edge.from_claim, CLAIM_TRUNCATE),
            edge.rel,
            edge.to_id,
            truncate_claim(&edge.to_claim, CLAIM_TRUNCATE),
            edge.depth,
        ));
    }
}

/// Render generic nodes/edges as an ASCII tree grouped by relationship.
pub fn render_ascii_graph(
    scope: &str,
    seed: Option<&str>,
    nodes: &[RenderNode],
    edges: &[RenderEdge],
) -> String {
    let mut out = String::new();
    out.push_str(&format!("scope: {scope}\n"));
    if let Some(s) = seed {
        out.push_str(&format!("seed: {s}\n"));
    }
    out.push_str(&format!(
        "nodes: {} | edges: {}\n",
        nodes.len(),
        edges.len()
    ));

    if edges.is_empty() {
        out.push_str("\n(no edges)\n");
        if !nodes.is_empty() {
            out.push_str("\nnodes:\n");
            for node in nodes {
                render_node_line(&mut out, node);
            }
        }
        return out;
    }

    let mut by_rel: BTreeMap<String, Vec<&RenderEdge>> = BTreeMap::new();
    for edge in edges {
        by_rel.entry(edge.rel.clone()).or_default().push(edge);
    }

    for (rel, rel_edges) in by_rel {
        out.push('\n');
        out.push_str(&format!("## {rel} ({} edges)\n", rel_edges.len()));
        let last = rel_edges.len().saturating_sub(1);
        for (i, edge) in rel_edges.iter().enumerate() {
            let branch = if i == last { "└─" } else { "├─" };
            let from = node_label(nodes, &edge.from_id);
            let to = node_label(nodes, &edge.to_id);
            out.push_str(&format!(
                "{branch} {} --{}--> {} (depth {})\n",
                from, edge.rel, to, edge.depth
            ));
        }
    }

    out.push_str("\nlegend: [id] claim\n");
    out
}

fn node_label(nodes: &[RenderNode], id: &str) -> String {
    nodes
        .iter()
        .find(|n| n.id == id)
        .map(|n| format!("[{}] {}", n.id, truncate_claim(&n.claim, CLAIM_TRUNCATE)))
        .unwrap_or_else(|| format!("[{id}]"))
}

fn render_node_line(out: &mut String, node: &RenderNode) {
    let seed = if node.is_seed { " *" } else { "" };
    let facet = if !node.collections.is_empty() || !node.tags.is_empty() {
        format!(
            " ({})",
            [node.collections.join(", "), node.tags.join(", ")]
                .into_iter()
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join("; ")
        )
    } else {
        String::new()
    };
    let dim = matches!(node.status.as_str(), "superseded" | "archived");
    let prefix = if dim {
        "~"
    } else if node.status == "unresolved_by_design" {
        "!"
    } else {
        " "
    };
    out.push_str(&format!(
        "{prefix} [{}] {}{} ({}, {}){}\n",
        node.id,
        truncate_claim(&node.claim, CLAIM_TRUNCATE),
        seed,
        facet,
        node.tier,
        node.status,
    ));
}

/// Render nodes/edges as Graphviz DOT.
pub fn render_dot(nodes: &[RenderNode], edges: &[RenderEdge]) -> String {
    let mut lines = vec![
        "digraph Alexandria {".to_string(),
        "  rankdir=LR;".to_string(),
        "  node [shape=box, fontsize=10];".to_string(),
    ];

    for node in nodes {
        let label = escape_dot_label(&format!(
            "{}\\n({})",
            truncate_claim(&node.claim, CLAIM_TRUNCATE),
            node.tier
        ));
        let id = dot_node_id(&node.id);
        let color = tier_dot_color(&node.tier);
        let mut attrs = vec![format!("label=\"{label}\""), format!("color=\"{color}\"")];
        if node.is_seed {
            attrs.push("penwidth=2".to_string());
        }
        if let Some(style) = status_dot_style(&node.status) {
            attrs.push(format!("style=\"{style}\""));
        }
        if matches!(node.status.as_str(), "superseded" | "archived") {
            attrs.push("fontcolor=gray".to_string());
        }
        if node.status == "unresolved_by_design" {
            attrs.push("fontname=Helvetica-Bold".to_string());
        }
        if !node.collections.is_empty() {
            attrs.push(format!(
                "tooltip=\"collections: {}\"",
                escape_dot_label(&node.collections.join(", "))
            ));
        }
        lines.push(format!("  {id} [{}];", attrs.join(", ")));
    }

    for edge in edges {
        let (style, color) = dot_edge_style(&edge.rel);
        let from = dot_node_id(&edge.from_id);
        let to = dot_node_id(&edge.to_id);
        let rel = escape_dot_label(&edge.rel.replace('_', " "));
        lines.push(format!(
            "  {from} -> {to} [label=\"{rel}\", style=\"{style}\", color=\"{color}\"];"
        ));
    }

    lines.push("}".to_string());
    lines.join("\n")
}

/// Render nodes/edges as a Mermaid flowchart.
pub fn render_mermaid(nodes: &[RenderNode], edges: &[RenderEdge]) -> String {
    let mut lines = vec!["flowchart LR".to_string()];
    let mut tiers_seen: BTreeSet<String> = BTreeSet::new();

    for node in nodes {
        let id = mermaid_node_id(&node.id);
        let label = escape_mermaid_label(&truncate_claim(&node.claim, CLAIM_TRUNCATE));
        lines.push(format!("  {id}[\"{label}\"]"));
        if !node.tier.is_empty() {
            tiers_seen.insert(node.tier.clone());
            lines.push(format!("  class {id} {}", tier_mermaid_class(&node.tier)));
        }
    }

    for edge in edges {
        let from = mermaid_node_id(&edge.from_id);
        let to = mermaid_node_id(&edge.to_id);
        let rel = escape_mermaid_label(&edge.rel.replace('_', " "));
        lines.push(format!("  {from} -->|{rel}| {to}"));
    }

    for tier in tiers_seen {
        let class = tier_mermaid_class(&tier);
        let fill = tier_dot_color(&tier);
        lines.push(format!("  classDef {class} fill:{fill},stroke:#333;"));
    }

    lines.join("\n")
}

/// Render a map result as Mermaid using claim labels.
pub fn render_map_mermaid(result: &MapResult, seed_ids: &[String]) -> String {
    let (nodes, edges) = from_map_result(result, seed_ids);
    render_mermaid(&nodes, &edges)
}

/// Render a provenance trace as ASCII.
pub fn render_trace_ascii(result: &TraceResult) -> String {
    let mut out = String::new();
    out.push_str(&format!("[{}] {}\n", result.id, result.claim));
    out.push_str(&format!(
        "confidence: {:.2} (effective: {:.2})\n",
        result.confidence, result.effective_confidence
    ));
    out.push_str(&format!(
        "derived_sources: {}\n",
        result.has_derived_sources
    ));

    if result.nodes.is_empty() {
        out.push_str("\n(no provenance nodes)\n");
        return out;
    }

    out.push_str("\nprovenance:\n");
    let mut by_depth: BTreeMap<u32, Vec<&TraceNode>> = BTreeMap::new();
    for node in &result.nodes {
        by_depth.entry(node.depth).or_default().push(node);
    }

    for (depth, nodes) in by_depth {
        out.push_str(&format!("\ndepth {depth}\n"));
        let last = nodes.len().saturating_sub(1);
        for (i, node) in nodes.iter().enumerate() {
            let branch = if i == last { "└─" } else { "├─" };
            out.push_str(&format!(
                "{branch} [{}] {} via {} -> {}{}\n",
                node.id,
                truncate_claim(&node.claim, CLAIM_TRUNCATE),
                node.source_kind,
                node.source_ref,
                node.observed
                    .as_ref()
                    .map(|o| format!(" (observed {o})"))
                    .unwrap_or_default()
            ));
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::{MapEdge, MapRelGroup, MapResult};

    #[test]
    fn truncate_claim_shortens_long_text() {
        let long = "a".repeat(60);
        let out = truncate_claim(&long, 48);
        assert!(out.chars().count() <= 48);
        assert!(out.ends_with('…'));
    }

    #[test]
    fn render_mermaid_uses_claim_labels() {
        let nodes = vec![RenderNode {
            id: "eng_a".into(),
            claim: "Auth uses JWT".into(),
            tier: "semantic".into(),
            status: "confirmed".into(),
            collections: vec![],
            tags: vec![],
            is_seed: true,
        }];
        let edges = vec![RenderEdge {
            from_id: "eng_a".into(),
            to_id: "eng_b".into(),
            rel: "supports".into(),
            depth: 1,
            kind: "typed".into(),
        }];
        let m = render_mermaid(&nodes, &edges);
        assert!(m.contains("flowchart LR"));
        assert!(m.contains("Auth uses JWT"));
        assert!(m.contains("supports"));
    }

    #[test]
    fn render_map_ascii_includes_rel_groups() {
        let result = MapResult {
            seed: "auth".into(),
            depth: 2,
            seed_ids: vec!["eng_a".into()],
            rel_groups: vec![MapRelGroup {
                rel: "supports".into(),
                token_cost: 10,
                edges: vec![MapEdge {
                    from_id: "eng_a".into(),
                    from_claim: "A".into(),
                    rel: "supports".into(),
                    to_id: "eng_b".into(),
                    to_claim: "B".into(),
                    depth: 1,
                    token_cost: 5,
                }],
            }],
            mermaid: String::new(),
            total_tokens: 10,
            edge_count: 1,
        };
        let ascii = render_map_ascii(&result);
        assert!(ascii.contains("supports"));
        assert!(ascii.contains("eng_a"));
    }
}
