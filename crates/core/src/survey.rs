//! Exhaustive-but-budgeted topic traversal: enumerate the relevant neighborhood
//! cheaply and surface where full bodies hide behind terse claims.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use serde::Serialize;

use crate::config::Config;
use crate::engram::{Engram, Tier};
use crate::error::Result;
use crate::facets::{detect_facets, facets_to_filters, DetectedFacet};
use crate::freshness::freshness_hint;
use crate::graph::Graph;
use crate::index::Index;
use crate::retrieval::{escape_fts_query, RecallOptions, RecallState, ResponseMode, Retrieval};

#[derive(Debug, Clone, Serialize)]
pub struct SurveyHit {
    pub id: String,
    pub claim: String,
    pub tier: String,
    pub status: String,
    pub claim_tokens: u32,
    pub body_tokens: u32,
    pub expansion_value: f64,
    pub signals: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub freshness_warning: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SurveyCollection {
    pub name: String,
    pub summary: String,
    pub token_cost: u32,
    pub hits: Vec<SurveyHit>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SurveyGap {
    pub facet_kind: String,
    pub facet_name: String,
    pub engram_count: usize,
    pub note: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SurveyResult {
    pub topic: String,
    pub state: RecallState,
    pub response_mode: ResponseMode,
    pub detected_facets: Vec<DetectedFacet>,
    pub collections: Vec<SurveyCollection>,
    pub gaps: Vec<SurveyGap>,
    pub total_tokens: u32,
    pub engram_count: u32,
}

pub struct SurveyOptions {
    pub depth: u32,
}

impl Default for SurveyOptions {
    fn default() -> Self {
        Self { depth: 2 }
    }
}

/// Run an exhaustive-but-budgeted survey of memory for a topic.
pub fn survey(
    topic: &str,
    index: &Index,
    config: &Config,
    budget: Option<u32>,
    options: SurveyOptions,
) -> Result<SurveyResult> {
    let budget = budget.unwrap_or(config.budgets.default_recall_tokens);
    let retrieval = Retrieval::new(index, config);
    let recall = retrieval.recall(topic, Some(budget), RecallOptions::default())?;

    let detected_facets = detect_facets(index, topic)?;
    let (facet_collections, facet_tags) = facets_to_filters(&detected_facets);

    let mut ids: BTreeSet<String> = recall
        .tree
        .collections
        .iter()
        .flat_map(|c| c.hits.iter().map(|h| h.id.clone()))
        .collect();

    for id in index.engram_ids_matching(&facet_collections, &facet_tags)? {
        ids.insert(id);
    }

    let fts_ids = index.fts_engram_ids(&escape_fts_query(topic), 500)?;
    for id in fts_ids {
        ids.insert(id);
    }

    // Graph expansion from seed hits
    let graph = Graph::new(index);
    let depth = options.depth.clamp(1, 3);
    let seed_ids: Vec<String> = ids.iter().take(10).cloned().collect();
    for seed in seed_ids {
        if let Ok(trav) = graph.traverse(&seed, None, depth) {
            for node in trav.nodes {
                if node.tier != "relational" {
                    ids.insert(node.id);
                }
            }
        }
    }

    ids.retain(|id| {
        index
            .get_engram(id)
            .ok()
            .flatten()
            .is_some_and(|row| row.tier != Tier::Relational)
    });

    let mut hits: Vec<SurveyHit> = Vec::new();
    for id in &ids {
        let Some(row) = index.get_engram(id)? else {
            continue;
        };
        let claim_tokens = Engram::estimate_tokens(&row.claim);
        let body_tokens = Engram::estimate_tokens(&row.body);
        let expansion_value = if claim_tokens > 0 {
            body_tokens as f64 / claim_tokens as f64
        } else {
            0.0
        };
        let freshness_warning = freshness_hint(index, &row.id, &config.freshness)?
            .and_then(|h| h.warning);
        hits.push(SurveyHit {
            id: row.id,
            claim: row.claim,
            tier: tier_label(row.tier).to_string(),
            status: status_label(row.status).to_string(),
            claim_tokens,
            body_tokens,
            expansion_value,
            signals: recall_signals_for(id, &recall),
            freshness_warning,
        });
    }

    hits.sort_by(|a, b| {
        b.expansion_value
            .partial_cmp(&a.expansion_value)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.id.cmp(&b.id))
    });

    let gaps = build_gaps(index, &detected_facets, &ids)?;
    let (collections, total_tokens) = assemble_survey_tree(index, &hits, budget);

    Ok(SurveyResult {
        topic: topic.to_string(),
        state: recall.state,
        response_mode: recall.response_mode,
        detected_facets,
        collections,
        gaps,
        total_tokens,
        engram_count: hits.len() as u32,
    })
}

fn recall_signals_for(id: &str, recall: &crate::retrieval::RecallResult) -> Vec<String> {
    for collection in &recall.tree.collections {
        if let Some(hit) = collection.hits.iter().find(|h| h.id == id) {
            return hit.signals.clone();
        }
    }
    vec!["survey".to_string()]
}

fn build_gaps(
    index: &Index,
    facets: &[DetectedFacet],
    included: &BTreeSet<String>,
) -> Result<Vec<SurveyGap>> {
    let mut gaps = Vec::new();
    for facet in facets {
        let (collections, tags) = match facet.kind {
            crate::facets::FacetKind::Collection => (vec![facet.name.clone()], vec![]),
            crate::facets::FacetKind::Tag => (vec![], vec![facet.name.clone()]),
        };
        let facet_ids: HashSet<String> = index
            .engram_ids_matching(&collections, &tags)?
            .into_iter()
            .collect();
        let missing: usize = facet_ids.iter().filter(|id| !included.contains(*id)).count();
        if missing > 0 {
            gaps.push(SurveyGap {
                facet_kind: match facet.kind {
                    crate::facets::FacetKind::Collection => "collection".to_string(),
                    crate::facets::FacetKind::Tag => "tag".to_string(),
                },
                facet_name: facet.name.clone(),
                engram_count: missing,
                note: format!(
                    "{missing} engram(s) in `{}` not in survey relevance set — touched but thin",
                    facet.name
                ),
            });
        }
    }
    Ok(gaps)
}

fn assemble_survey_tree(
    index: &Index,
    hits: &[SurveyHit],
    budget: u32,
) -> (Vec<SurveyCollection>, u32) {
    let mut by_collection: BTreeMap<String, Vec<&SurveyHit>> = BTreeMap::new();
    for hit in hits {
        let collection = index
            .get_engram(&hit.id)
            .ok()
            .flatten()
            .and_then(|row| row.collections.first().cloned())
            .unwrap_or_else(|| "_uncategorized".to_string());
        by_collection.entry(collection).or_default().push(hit);
    }

    let mut collections = Vec::new();
    let mut total_tokens = 0u32;

    for (name, group) in by_collection {
        let display_name = if name == "_uncategorized" {
            "(uncategorized)".to_string()
        } else {
            name.clone()
        };
        let summary = format!("{} — {} engram(s)", display_name, group.len());
        let summary_cost = Engram::estimate_tokens(&summary);
        if total_tokens + summary_cost > budget && !collections.is_empty() {
            break;
        }
        let mut node_cost = summary_cost;
        let mut node_hits = Vec::new();
        for hit in group {
            let hit_cost = hit.claim_tokens + 1; // claim line overhead
            if total_tokens + node_cost + hit_cost > budget && !node_hits.is_empty() {
                break;
            }
            node_cost += hit_cost;
            node_hits.push((*hit).clone());
        }
        if node_hits.is_empty() && total_tokens + summary_cost > budget {
            break;
        }
        total_tokens += node_cost;
        collections.push(SurveyCollection {
            name,
            summary,
            token_cost: node_cost,
            hits: node_hits,
        });
    }

    (collections, total_tokens)
}

fn tier_label(tier: crate::engram::Tier) -> &'static str {
    match tier {
        crate::engram::Tier::Working => "working",
        crate::engram::Tier::Episodic => "episodic",
        crate::engram::Tier::Provisional => "provisional",
        crate::engram::Tier::Semantic => "semantic",
        crate::engram::Tier::Procedural => "procedural",
        crate::engram::Tier::Relational => "relational",
    }
}

fn status_label(status: crate::engram::Status) -> &'static str {
    match status {
        crate::engram::Status::Confirmed => "confirmed",
        crate::engram::Status::Provisional => "provisional",
        crate::engram::Status::UnresolvedByDesign => "unresolved_by_design",
        crate::engram::Status::Superseded => "superseded",
        crate::engram::Status::Archived => "archived",
    }
}
