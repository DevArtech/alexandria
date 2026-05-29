use std::collections::{HashMap, HashSet};

use rusqlite::params;
use serde::Serialize;

use crate::config::{Config, ThresholdsConfig};
use crate::engram::{Engram, Rel, Tier};
use crate::error::Result;
use crate::graph::compute_effective_confidence;
use crate::index::Index;
/// Options that influence response-mode selection on `recall`.
#[derive(Debug, Clone, Default)]
pub struct RecallOptions {
    pub audit: bool,
    pub high_stakes: bool,
    /// Domain for meta-memory lookup (first collection when None).
    pub domain: Option<String>,
}

/// Inputs to the rule-based posture judge (ARCHITECTURE §10.1).
#[derive(Debug, Clone)]
struct PostureInputs {
    state: RecallState,
    has_provisional: bool,
    has_conflict_edges: bool,
    audit_requested: bool,
    high_stakes: bool,
    meta_reliability_weak: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RecallState {
    StrongHit,
    WeakHit,
    HighConfidenceGap,
    LowConfidenceGap,
    Nothing,
}

impl RecallState {
    pub fn as_str(self) -> &'static str {
        match self {
            RecallState::StrongHit => "strong_hit",
            RecallState::WeakHit => "weak_hit",
            RecallState::HighConfidenceGap => "high_confidence_gap",
            RecallState::LowConfidenceGap => "low_confidence_gap",
            RecallState::Nothing => "nothing",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResponseMode {
    Flow,
    Humility,
    Audit,
}

impl ResponseMode {
    pub fn as_str(self) -> &'static str {
        match self {
            ResponseMode::Flow => "flow",
            ResponseMode::Humility => "humility",
            ResponseMode::Audit => "audit",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct RecallHit {
    pub id: String,
    pub claim: String,
    pub tier: String,
    pub status: String,
    pub score: f64,
    pub confidence: f64,
    pub effective_confidence: f64,
    pub token_cost: u32,
    pub signals: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CollectionNode {
    pub name: String,
    pub summary: String,
    pub token_cost: u32,
    pub hits: Vec<RecallHit>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ContextTree {
    pub collections: Vec<CollectionNode>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RecallResult {
    pub state: RecallState,
    pub response_mode: ResponseMode,
    pub query: String,
    pub tree: ContextTree,
    pub total_tokens: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct LinkClaim {
    pub rel: String,
    pub to_id: String,
    pub claim: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExpandResult {
    pub id: String,
    pub claim: String,
    pub tier: String,
    pub status: String,
    pub body: String,
    pub confidence: f64,
    pub effective_confidence: f64,
    pub token_cost: u32,
    pub links: Vec<LinkClaim>,
}

pub struct Retrieval<'a> {
    index: &'a Index,
    config: &'a Config,
}

#[derive(Debug, Clone)]
struct FusedHit {
    id: String,
    claim: String,
    tier: String,
    status: String,
    score: f64,
    signals: Vec<String>,
    collections: Vec<String>,
}

impl<'a> Retrieval<'a> {
    pub fn new(index: &'a Index, config: &'a Config) -> Self {
        Self { index, config }
    }

    pub fn recall(
        &self,
        query: &str,
        budget: Option<u32>,
        options: RecallOptions,
    ) -> Result<RecallResult> {
        let budget = budget.unwrap_or(self.config.budgets.default_recall_tokens);
        let thresholds = &self.config.thresholds;
        let fts_query = escape_fts_query(query);

        let candidate_limit = candidate_limit(budget);
        let lexical = self.search_fts(&fts_query, candidate_limit)?;
        let query_vec = self.index.embed_query(query)?;
        let semantic = self.index.semantic_knn(&query_vec, candidate_limit)?;

        let shape_hits = if self.config.shape.enabled {
            self.search_shape(&query_vec, candidate_limit)?
        } else {
            Vec::new()
        };

        let semantic_distances: HashMap<String, f32> = semantic
            .iter()
            .map(|h| (h.id.clone(), h.distance as f32))
            .collect();
        let best_semantic_distance = semantic.first().map(|h| h.distance as f32);
        let has_lexical_match = !lexical.is_empty();

        let shape_weight = self.config.shape.weight;
        let mut fused = fuse_rrf_multi(
            &[
                ("lexical", 1.0, lexical_to_rrf_entries(&lexical)),
                (
                    "semantic",
                    1.0,
                    semantic_to_rrf_entries(&semantic),
                ),
                (
                    "shape",
                    shape_weight,
                    shape_to_rrf_entries(&shape_hits),
                ),
            ],
            thresholds.rrf_k,
        );
        for hit in &mut fused {
            if hit.collections.is_empty() {
                hit.collections = self.fetch_collections(&hit.id).unwrap_or_default();
            }
        }

        let neighbor_count = self
            .index
            .neighbors_within(&query_vec, thresholds.density_radius)?;
        let centroid_near = self
            .index
            .nearest_collection_centroid(&query_vec)?
            .map(|(_, dist)| dist < thresholds.centroid_radius)
            .unwrap_or(false);

        let state = classify_state(
            &fused,
            &semantic_distances,
            has_lexical_match,
            best_semantic_distance,
            thresholds,
            neighbor_count,
            centroid_near,
        );

        let fused_ids: Vec<String> = fused.iter().map(|h| h.id.clone()).collect();
        let fused_statuses: Vec<(String, String)> = fused
            .iter()
            .map(|h| (h.id.clone(), h.status.clone()))
            .collect();
        let domain = options.domain.clone().or_else(|| {
            fused
                .first()
                .and_then(|h| h.collections.first().cloned())
        });

        apply_rerank_if_enabled(self.index, self.config, query, &mut fused)?;
        // Rerank runs after classify_state so five-state bands stay on RRF/semantic signals.

        // Bounded M5 calibration: down-weight fused scores in low-reliability domains.
        // Full live per-domain threshold self-tuning is intentionally deferred.
        if self.config.calibration.enabled {
            if let Some(d) = &domain {
                let reliability = self.index.meta_reliability(Some(d.as_str()))?;
                if reliability < self.config.posture.meta_reliability_threshold {
                    let weight = reliability.clamp(self.config.calibration.score_weight_floor, 1.0);
                    for hit in &mut fused {
                        hit.score *= weight;
                    }
                    fused.sort_by(|a, b| {
                        b.score
                            .partial_cmp(&a.score)
                            .unwrap_or(std::cmp::Ordering::Equal)
                    });
                }
            }
        }

        let meta_weak = match &domain {
            Some(d) => {
                self.index.meta_reliability(Some(d.as_str()))?
                    < self.config.posture.meta_reliability_threshold
            }
            None => false,
        };

        let response_mode = judge_posture(PostureInputs {
            state,
            has_provisional: fused_has_provisional(&fused_statuses),
            has_conflict_edges: self.index.has_conflict_edges_among(&fused_ids)?,
            audit_requested: options.audit,
            high_stakes: options.high_stakes,
            meta_reliability_weak: meta_weak,
        });

        let tree = build_context_tree(
            self.index,
            &fused,
            &semantic_distances,
            has_lexical_match,
            budget,
            state,
            thresholds,
        );
        let total_tokens = tree_total_tokens(&tree);

        Ok(RecallResult {
            state,
            response_mode,
            query: query.to_string(),
            tree,
            total_tokens,
        })
    }

    pub fn expand(&self, id: &str, rel: Option<Rel>) -> Result<ExpandResult> {
        let row = self
            .index
            .get_engram(id)?
            .ok_or_else(|| crate::error::AlexandriaError::EngramNotFound(id.to_string()))?;

        if row.tier == Tier::Relational {
            return Err(crate::error::AlexandriaError::InvalidEngram(
                "relational engrams cannot be expanded as quotable text".into(),
            ));
        }

        let links = self.index.get_linked_claims(id, rel)?;
        let link_claims: Vec<LinkClaim> = links
            .into_iter()
            .map(|(r, to_id, claim)| LinkClaim {
                rel: rel_label(r).to_string(),
                to_id,
                claim,
            })
            .collect();

        let engram = self.load_engram_for_confidence(id, &row)?;
        let effective = compute_effective_confidence(self.index, &engram)?;

        let token_cost = Engram::estimate_tokens(&row.body);

        Ok(ExpandResult {
            id: row.id,
            claim: row.claim,
            tier: tier_label(row.tier).to_string(),
            status: status_label(row.status).to_string(),
            body: row.body,
            confidence: engram.confidence,
            effective_confidence: effective,
            token_cost,
            links: link_claims,
        })
    }

    fn load_engram_for_confidence(&self, id: &str, row: &crate::index::EngramRow) -> Result<Engram> {
        if let Some(path) = self.index.file_path(id)? {
            if let Ok(content) = std::fs::read_to_string(&path) {
                if let Ok(engram) = Engram::parse(&content) {
                    return Ok(engram);
                }
            }
        }
        Ok(Engram {
            id: row.id.clone(),
            tier: row.tier,
            status: row.status,
            claim: row.claim.clone(),
            created: chrono::Utc::now(),
            updated: chrono::Utc::now(),
            last_touched: chrono::Utc::now(),
            source: Vec::new(),
            confidence: row.confidence,
            salience: 0.7,
            collections: row.collections.clone(),
            tags: Vec::new(),
            links: row
                .links
                .iter()
                .map(|(rel, to)| crate::engram::Link {
                    rel: *rel,
                    to: to.clone(),
                })
                .collect(),
            embedding_ref: None,
            shape_ref: None,
            surface_when: None,
            output_policy: None,
            body: row.body.clone(),
        })
    }

    fn search_fts(&self, fts_query: &str, limit: u32) -> Result<Vec<FusedHit>> {
        let conn = self.index.connection();
        let mut stmt = conn.prepare(
            r#"
            SELECT e.id, e.claim, e.tier, e.status, bm25(engrams_fts) AS score
            FROM engrams_fts
            JOIN engrams e ON e.rowid = engrams_fts.rowid
            WHERE engrams_fts MATCH ?1
              AND e.tier != 'relational'
            ORDER BY score ASC
            LIMIT ?2
            "#,
        )?;

        let rows = stmt.query_map(params![fts_query, limit], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?;

        let mut hits = Vec::new();
        for row in rows {
            let (id, claim, tier, status) = row?;
            let collections = self.fetch_collections(&id)?;
            hits.push(FusedHit {
                id,
                claim,
                tier,
                status,
                score: 0.0,
                signals: vec!["lexical".into()],
                collections,
            });
        }
        Ok(hits)
    }

    fn fetch_collections(&self, engram_id: &str) -> Result<Vec<String>> {
        let conn = self.index.connection();
        let mut stmt = conn.prepare(
            "SELECT collection FROM collection_members WHERE engram_id = ?1",
        )?;
        let rows = stmt.query_map(params![engram_id], |row| row.get(0))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    fn search_shape(
        &self,
        query_vec: &[f32],
        limit: u32,
    ) -> Result<Vec<crate::index::SemanticHit>> {
        let hits = self.index.shape_knn(query_vec, limit)?;
        let max_dist = self.config.shape.max_distance;
        Ok(hits
            .into_iter()
            .filter(|h| (h.distance as f32) <= max_dist)
            .collect())
    }
}

#[derive(Debug, Clone)]
struct RrfEntry {
    id: String,
    claim: String,
    tier: String,
    status: String,
    collections: Vec<String>,
}

fn lexical_to_rrf_entries(lexical: &[FusedHit]) -> Vec<RrfEntry> {
    lexical
        .iter()
        .map(|h| RrfEntry {
            id: h.id.clone(),
            claim: h.claim.clone(),
            tier: h.tier.clone(),
            status: h.status.clone(),
            collections: h.collections.clone(),
        })
        .collect()
}

fn semantic_to_rrf_entries(semantic: &[crate::index::SemanticHit]) -> Vec<RrfEntry> {
    semantic
        .iter()
        .map(|h| RrfEntry {
            id: h.id.clone(),
            claim: h.claim.clone(),
            tier: h.tier.clone(),
            status: h.status.clone(),
            collections: Vec::new(),
        })
        .collect()
}

fn shape_to_rrf_entries(shape: &[crate::index::SemanticHit]) -> Vec<RrfEntry> {
    semantic_to_rrf_entries(shape)
}

fn fuse_rrf_multi(
    lists: &[(&str, f64, Vec<RrfEntry>)],
    k: u32,
) -> Vec<FusedHit> {
    let kf = k as f64;
    let mut by_id: HashMap<String, FusedHit> = HashMap::new();
    let mut signal_sets: HashMap<String, HashSet<String>> = HashMap::new();

    for (signal, weight, entries) in lists {
        if entries.is_empty() || *weight <= 0.0 {
            continue;
        }
        for (rank, hit) in entries.iter().enumerate() {
            let rrf = weight * (1.0 / (kf + (rank + 1) as f64));
            let entry = by_id.entry(hit.id.clone()).or_insert_with(|| FusedHit {
                id: hit.id.clone(),
                claim: hit.claim.clone(),
                tier: hit.tier.clone(),
                status: hit.status.clone(),
                score: 0.0,
                signals: Vec::new(),
                collections: hit.collections.clone(),
            });
            entry.score += rrf;
            signal_sets
                .entry(hit.id.clone())
                .or_default()
                .insert(signal.to_string());
        }
    }

    let mut fused: Vec<FusedHit> = by_id.into_values().collect();
    for hit in &mut fused {
        if let Some(sigs) = signal_sets.get(&hit.id) {
            hit.signals = sigs.iter().cloned().collect();
            hit.signals.sort();
        }
    }
    fused.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    fused
}

/// Backward-compatible 2-list fusion for tests.
#[cfg(test)]
fn fuse_rrf(
    lexical: &[FusedHit],
    semantic: &[crate::index::SemanticHit],
    k: u32,
) -> Vec<FusedHit> {
    fuse_rrf_multi(
        &[
            ("lexical", 1.0, lexical_to_rrf_entries(lexical)),
            ("semantic", 1.0, semantic_to_rrf_entries(semantic)),
        ],
        k,
    )
}

fn apply_rerank_if_enabled(
    index: &Index,
    config: &Config,
    query: &str,
    fused: &mut [FusedHit],
) -> Result<()> {
    if !config.reranker.enabled || fused.is_empty() {
        return Ok(());
    }
    index.with_reranker(|reranker| {
        if let Some(r) = reranker {
            rerank_fused_hits(fused, query, r, config.reranker.top_n as usize)?;
        }
        Ok(())
    })
}

fn rerank_fused_hits(
    fused: &mut [FusedHit],
    query: &str,
    reranker: &dyn crate::provider::Reranker,
    top_n: usize,
) -> Result<()> {
    let n = top_n.min(fused.len());
    if n == 0 {
        return Ok(());
    }
    let docs: Vec<String> = fused.iter().take(n).map(|h| h.claim.clone()).collect();
    let scores = reranker.rerank(query, &docs)?;
    let mut order: Vec<(usize, f32)> = (0..n).zip(scores).collect();
    order.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let reordered: Vec<FusedHit> = order.into_iter().map(|(i, _)| fused[i].clone()).collect();
    for (i, hit) in reordered.into_iter().enumerate() {
        fused[i] = hit;
    }
    Ok(())
}

fn hit_semantically_relevant(
    hit: &FusedHit,
    semantic_distances: &HashMap<String, f32>,
    weak_max: f32,
) -> bool {
    semantic_distances
        .get(&hit.id)
        .map(|&d| d <= weak_max)
        .unwrap_or(false)
}

fn hit_is_relevant(
    hit: &FusedHit,
    semantic_distances: &HashMap<String, f32>,
    _has_lexical_match: bool,
    thresholds: &ThresholdsConfig,
) -> bool {
    hit.signals.iter().any(|s| s == "lexical")
        || hit_semantically_relevant(hit, semantic_distances, thresholds.semantic_weak_max_distance)
}

fn classify_state(
    fused: &[FusedHit],
    semantic_distances: &HashMap<String, f32>,
    has_lexical_match: bool,
    best_semantic_distance: Option<f32>,
    thresholds: &ThresholdsConfig,
    neighbor_count: u32,
    centroid_near: bool,
) -> RecallState {
    let semantically_relevant = best_semantic_distance
        .map(|d| d <= thresholds.semantic_weak_max_distance)
        .unwrap_or(false);
    let any_relevant = has_lexical_match || semantically_relevant;

    if !any_relevant {
        if neighbor_count >= thresholds.density_min_count {
            return RecallState::HighConfidenceGap;
        }
        if centroid_near {
            return RecallState::LowConfidenceGap;
        }
        return RecallState::Nothing;
    }

    let top = fused
        .iter()
        .find(|h| hit_is_relevant(h, semantic_distances, has_lexical_match, thresholds));

    let Some(top) = top else {
        return RecallState::WeakHit;
    };

    let top_distance = semantic_distances.get(&top.id).copied();
    let sem_strong = top_distance
        .map(|d| d <= thresholds.semantic_strong_max_distance)
        .unwrap_or(false);
    let has_lexical = top.signals.iter().any(|s| s == "lexical");

    if top.score >= thresholds.strong_cutoff
        && top.signals.len() >= thresholds.min_corroborating_signals as usize
        && (has_lexical || sem_strong)
    {
        return RecallState::StrongHit;
    }

    RecallState::WeakHit
}

fn judge_posture(inputs: PostureInputs) -> ResponseMode {
    if inputs.audit_requested || inputs.high_stakes {
        return ResponseMode::Audit;
    }
    let humility = matches!(
        inputs.state,
        RecallState::WeakHit | RecallState::HighConfidenceGap | RecallState::LowConfidenceGap
    ) || inputs.has_provisional
        || inputs.has_conflict_edges
        || inputs.meta_reliability_weak;
    if humility {
        ResponseMode::Humility
    } else {
        ResponseMode::Flow
    }
}

fn fused_has_provisional(hits: &[(String, String)]) -> bool {
    hits.iter().any(|(_, status)| status == "provisional")
}

fn hit_confidence(index: &Index, id: &str, status: &str) -> (f64, f64) {
    let confidence = index
        .get_engram(id)
        .ok()
        .flatten()
        .map(|r| r.confidence)
        .unwrap_or(0.9);
    let mut effective = if let (Ok(Some(path)), Ok(sources)) =
        (index.file_path(id), index.get_sources(id))
    {
        if let Ok(content) = std::fs::read_to_string(&path) {
            if let Ok(mut engram) = Engram::parse(&content) {
                engram.source = sources;
                compute_effective_confidence(index, &engram).unwrap_or(confidence)
            } else {
                confidence
            }
        } else {
            confidence
        }
    } else {
        confidence
    };
    if status == "provisional" {
        effective = effective.min(0.75);
    }
    (confidence, effective)
}

fn build_context_tree(
    index: &Index,
    fused: &[FusedHit],
    semantic_distances: &HashMap<String, f32>,
    has_lexical_match: bool,
    budget: u32,
    state: RecallState,
    thresholds: &ThresholdsConfig,
) -> ContextTree {
    if state == RecallState::Nothing
        || state == RecallState::HighConfidenceGap
        || state == RecallState::LowConfidenceGap
    {
        return ContextTree {
            collections: Vec::new(),
        };
    }

    let qualifying: Vec<_> = fused
        .iter()
        .filter(|h| {
            h.score >= thresholds.weak_cutoff
                && hit_is_relevant(h, semantic_distances, has_lexical_match, thresholds)
        })
        .collect();

    let mut groups: HashMap<String, Vec<RecallHit>> = HashMap::new();
    for hit in qualifying {
        let collection = hit
            .collections
            .first()
            .cloned()
            .unwrap_or_else(|| "_uncategorized".to_string());
        let (confidence, effective) = hit_confidence(index, &hit.id, hit.status.as_str());
        let recall_hit = RecallHit {
            id: hit.id.clone(),
            claim: hit.claim.clone(),
            tier: hit.tier.clone(),
            status: hit.status.clone(),
            score: hit.score,
            confidence,
            effective_confidence: effective,
            token_cost: 0,
            signals: hit.signals.clone(),
        };
        groups.entry(collection).or_default().push(recall_hit);
    }

    let mut collection_names: Vec<String> = groups.keys().cloned().collect();
    collection_names.sort();

    let mut nodes = Vec::new();
    let mut total_tokens = 0u32;

    for name in collection_names {
        let hits = groups.get(&name).unwrap();
        let display_name = if name == "_uncategorized" {
            "(uncategorized)".to_string()
        } else {
            name.clone()
        };
        let summary = format!("{} — {} engram(s)", display_name, hits.len());
        let summary_cost = Engram::estimate_tokens(&summary);

        let mut node_hits = Vec::new();
        let mut node_token_cost = summary_cost;

        if total_tokens + summary_cost > budget && !nodes.is_empty() {
            break;
        }
        total_tokens += summary_cost;

        for mut hit in hits.clone() {
            let claim_cost = Engram::estimate_tokens(&hit.claim);
            if total_tokens + claim_cost > budget && !node_hits.is_empty() {
                break;
            }
            hit.token_cost = claim_cost;
            total_tokens += claim_cost;
            node_token_cost += claim_cost;
            node_hits.push(hit);
            if total_tokens >= budget {
                break;
            }
        }

        if !node_hits.is_empty() || nodes.is_empty() {
            nodes.push(CollectionNode {
                name: display_name,
                summary,
                token_cost: node_token_cost,
                hits: node_hits,
            });
        }
        if total_tokens >= budget {
            break;
        }
    }

    ContextTree {
        collections: nodes,
    }
}

/// Cap KNN candidate pool (sqlite-vec max k is 4096).
fn candidate_limit(budget: u32) -> u32 {
    budget.saturating_mul(2).clamp(10, 200)
}

fn tree_total_tokens(tree: &ContextTree) -> u32 {
    tree.collections
        .iter()
        .map(|c| c.token_cost)
        .sum()
}

/// Escape a user query for FTS5 MATCH.
pub fn escape_fts_query(query: &str) -> String {
    query
        .split_whitespace()
        .filter(|t| !t.is_empty())
        .map(|token| {
            let needs_quote = token.chars().any(|c| !c.is_alphanumeric() && c != '_')
                || token.eq_ignore_ascii_case("AND")
                || token.eq_ignore_ascii_case("OR")
                || token.eq_ignore_ascii_case("NOT")
                || token.eq_ignore_ascii_case("NEAR");
            if needs_quote {
                format!("\"{}\"", token.replace('"', "\"\""))
            } else {
                token.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn tier_label(tier: Tier) -> &'static str {
    match tier {
        Tier::Working => "working",
        Tier::Episodic => "episodic",
        Tier::Provisional => "provisional",
        Tier::Semantic => "semantic",
        Tier::Procedural => "procedural",
        Tier::Relational => "relational",
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

fn rel_label(rel: Rel) -> &'static str {
    match rel {
        Rel::Supports => "supports",
        Rel::Refines => "refines",
        Rel::DependsOn => "depends_on",
        Rel::CausedBy => "caused_by",
        Rel::ConflictsConfirmed => "conflicts_confirmed",
        Rel::TensionPossible => "tension_possible",
        Rel::ContextQualified => "context_qualified",
        Rel::Coexists => "coexists",
        Rel::Supersedes => "supersedes",
        Rel::SupersededBy => "superseded_by",
        Rel::AspectOf => "aspect_of",
        Rel::SameEpisode => "same_episode",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::build_embedder;
    use crate::{Config, Index, Library, Status, Tier};
    use tempfile::TempDir;

    /// Hash embedder L2 distances are much larger than fastembed; use relaxed cutoffs in tests.
    fn setup() -> (TempDir, Library, Index, Config) {
        setup_with_semantic_thresholds(1.3, 1.15)
    }

    fn setup_with_semantic_thresholds(weak_max: f32, strong_max: f32) -> (TempDir, Library, Index, Config) {
        let dir = TempDir::new().unwrap();
        let lib = Library::init(dir.path()).unwrap();
        let mut config = Config::load(dir.path()).unwrap();
        config.providers.embedder = "hash".into();
        config.thresholds.semantic_weak_max_distance = weak_max;
        config.thresholds.semantic_strong_max_distance = strong_max;
        let embedder = build_embedder(&config).unwrap();
        let index = Index::open_with_embedder(&lib, embedder).unwrap();
        (dir, lib, index, config)
    }

    fn remember(lib: &Library, index: &Index, claim: &str, body: &str) {
        remember_with_collections(lib, index, claim, body, &[]);
    }

    fn remember_with_collections(
        lib: &Library,
        index: &Index,
        claim: &str,
        body: &str,
        collections: &[&str],
    ) {
        let mut e = Engram::new(claim, body, Tier::Semantic, Status::Confirmed);
        e.collections = collections.iter().map(|s| (*s).to_string()).collect();
        let p = lib.write_engram(&e).unwrap();
        index.upsert(&e, &p.display().to_string()).unwrap();
    }

    #[test]
    fn escape_fts_query_quotes_special_tokens() {
        assert_eq!(escape_fts_query("C++"), "\"C++\"");
        assert_eq!(escape_fts_query("foo AND bar"), "foo \"AND\" bar");
    }

    #[test]
    fn rrf_fusion_combines_signals() {
        let lexical = vec![FusedHit {
            id: "a".into(),
            claim: "c".into(),
            tier: "semantic".into(),
            status: "confirmed".into(),
            score: 0.0,
            signals: vec!["lexical".into()],
            collections: vec![],
        }];
        let semantic = vec![crate::index::SemanticHit {
            id: "a".into(),
            claim: "c".into(),
            tier: "semantic".into(),
            status: "confirmed".into(),
            distance: 0.1,
        }];
        let fused = fuse_rrf(&lexical, &semantic, 60);
        assert_eq!(fused[0].id, "a");
        assert!(fused[0].score > 0.0);
        assert_eq!(fused[0].signals.len(), 2);
    }

    #[test]
    fn classify_state_strong_requires_corroboration() {
        let thresholds = ThresholdsConfig::default();
        let strong = [FusedHit {
            id: "a".into(),
            claim: "c".into(),
            tier: "semantic".into(),
            status: "confirmed".into(),
            score: 0.05,
            signals: vec!["lexical".into(), "semantic".into()],
            collections: vec![],
        }];
        let mut dist = HashMap::new();
        dist.insert("a".into(), 0.1f32);
        assert_eq!(
            classify_state(&strong, &dist, true, Some(0.1), &thresholds, 0, false),
            RecallState::StrongHit
        );
    }

    #[test]
    fn hybrid_recall_finds_semantic_match() {
        let (_dir, lib, index, config) = setup();
        remember(
            &lib,
            &index,
            "database connection pooling under heavy load",
            "pgbouncer and transaction timeouts",
        );

        let retrieval = Retrieval::new(&index, &config);
        let result = retrieval
            .recall("connection pool exhaustion", Some(2000), RecallOptions::default())
            .unwrap();

        assert_ne!(result.state, RecallState::Nothing);
        let all_hits: Vec<_> = result
            .tree
            .collections
            .iter()
            .flat_map(|c| c.hits.iter())
            .collect();
        assert!(!all_hits.is_empty());
    }

    #[test]
    fn expand_returns_body_and_links() {
        let (_dir, lib, index, config) = setup();
        let mut e1 = Engram::new("parent claim", "parent body", Tier::Semantic, Status::Confirmed);
        let e2 = Engram::new("child claim", "child body", Tier::Semantic, Status::Confirmed);
        e1.links.push(crate::engram::Link {
            rel: Rel::Supports,
            to: e2.id.clone(),
        });
        let p1 = lib.write_engram(&e1).unwrap();
        let p2 = lib.write_engram(&e2).unwrap();
        index.upsert(&e1, &p1.display().to_string()).unwrap();
        index.upsert(&e2, &p2.display().to_string()).unwrap();

        let retrieval = Retrieval::new(&index, &config);
        let expanded = retrieval.expand(&e1.id, None).unwrap();
        assert_eq!(expanded.body, "parent body");
        assert_eq!(expanded.links.len(), 1);
        assert_eq!(expanded.links[0].claim, "child claim");
    }

    #[test]
    fn high_confidence_gap_classified_with_dense_neighborhood() {
        let thresholds = ThresholdsConfig {
            density_min_count: 3,
            semantic_weak_max_distance: 0.5,
            density_radius: 0.8,
            ..ThresholdsConfig::default()
        };
        let weak = [FusedHit {
            id: "a".into(),
            claim: "c".into(),
            tier: "semantic".into(),
            status: "confirmed".into(),
            score: 0.01,
            signals: vec!["semantic".into()],
            collections: vec![],
        }];
        let mut dist = HashMap::new();
        dist.insert("a".into(), 0.9f32);
        assert_eq!(
            classify_state(&weak, &dist, false, Some(0.9), &thresholds, 5, false),
            RecallState::HighConfidenceGap
        );
    }

    #[test]
    fn low_confidence_gap_classified_near_centroid() {
        let thresholds = ThresholdsConfig {
            semantic_weak_max_distance: 0.5,
            ..ThresholdsConfig::default()
        };
        let weak = [FusedHit {
            id: "a".into(),
            claim: "c".into(),
            tier: "semantic".into(),
            status: "confirmed".into(),
            score: 0.01,
            signals: vec!["semantic".into()],
            collections: vec![],
        }];
        let mut dist = HashMap::new();
        dist.insert("a".into(), 0.9f32);
        assert_eq!(
            classify_state(&weak, &dist, false, Some(0.9), &thresholds, 0, true),
            RecallState::LowConfidenceGap
        );
    }

    #[test]
    fn hash_embedder_distance_sanity() {
        use crate::provider::{embed_sync, HashEmbedder};
        let e = HashEmbedder;
        let related_a = embed_sync(&e, &["database connection pooling under heavy load".into()]).unwrap()[0].clone();
        let related_b = embed_sync(&e, &["connection pool exhaustion".into()]).unwrap()[0].clone();
        let unrelated = embed_sync(&e, &["xylophone quasar nebula".into()]).unwrap()[0].clone();
        fn l2(a: &[f32], b: &[f32]) -> f32 {
            a.iter().zip(b).map(|(x, y)| (x - y) * (x - y)).sum::<f32>().sqrt()
        }
        let d_related = l2(&related_a, &related_b);
        let d_unrelated = l2(&related_a, &unrelated);
        eprintln!("hash L2 related={d_related} unrelated={d_unrelated}");
        assert!(d_related < d_unrelated);
        assert!(d_related < 1.3);
        assert!(d_unrelated > 1.3);
    }

    #[test]
    fn recall_dense_cluster_yields_high_confidence_gap() {
        let weak = 1.25f32;
        let density = 1.55f32;
        let (_dir, lib, index, mut config) = setup_with_semantic_thresholds(weak, 1.1);
        config.thresholds.density_radius = density;
        config.thresholds.density_min_count = 3;

        let cluster = "kubernetes pod scheduling affinity rule cluster autoscaler node pool";
        for i in 0..6 {
            remember(
                &lib,
                &index,
                &format!("{cluster} configuration variant {i}"),
                "cluster autoscaler node pool configuration tuning",
            );
        }

        let retrieval = Retrieval::new(&index, &config);
        let query = "orchestra symphony violin concert hall performance";
        let result = retrieval
            .recall(query, Some(2000), RecallOptions::default())
            .unwrap();

        assert_eq!(
            result.state,
            RecallState::HighConfidenceGap,
            "expected high_confidence_gap for query '{query}' against dense cluster; got {:?}",
            result.state
        );
        assert!(result.tree.collections.is_empty());
        assert_eq!(result.response_mode, ResponseMode::Humility);
    }

    #[test]
    fn recall_nonsense_query_yields_gap_or_nothing() {
        let (_dir, lib, index, config) = setup_with_semantic_thresholds(1.3, 1.15);
        remember(
            &lib,
            &index,
            "PostgreSQL pgbouncer deadlock transaction timeouts",
            "Connection pool exhaustion under load.",
        );
        remember(
            &lib,
            &index,
            "Alexandria uses hybrid fused retrieval",
            "Vector-only retrieval fails on exact recall.",
        );
        remember(
            &lib,
            &index,
            "Rust is the target runtime for Alexandria",
            "Single binary, local-first.",
        );

        let retrieval = Retrieval::new(&index, &config);
        let result = retrieval
            .recall(
                "xylophone quasar nebula zzztqx",
                Some(2000),
                RecallOptions::default(),
            )
            .unwrap();

        assert!(
            matches!(
                result.state,
                RecallState::HighConfidenceGap
                    | RecallState::LowConfidenceGap
                    | RecallState::Nothing
            ),
            "expected gap or nothing, got {:?}",
            result.state
        );
        assert!(result.tree.collections.is_empty());
    }

    #[test]
    fn posture_judge_flow_on_strong_hit() {
        assert_eq!(
            judge_posture(PostureInputs {
                state: RecallState::StrongHit,
                has_provisional: false,
                has_conflict_edges: false,
                audit_requested: false,
                high_stakes: false,
                meta_reliability_weak: false,
            }),
            ResponseMode::Flow
        );
    }

    #[test]
    fn posture_judge_humility_on_provisional() {
        assert_eq!(
            judge_posture(PostureInputs {
                state: RecallState::StrongHit,
                has_provisional: true,
                has_conflict_edges: false,
                audit_requested: false,
                high_stakes: false,
                meta_reliability_weak: false,
            }),
            ResponseMode::Humility
        );
    }

    #[test]
    fn expand_refuses_relational() {
        let (_dir, lib, index, config) = setup();
        let e = Engram::new("prefers terse", "body", Tier::Relational, Status::Confirmed);
        let p = lib.write_engram(&e).unwrap();
        index.upsert(&e, &p.display().to_string()).unwrap();

        let retrieval = Retrieval::new(&index, &config);
        assert!(retrieval.expand(&e.id, None).is_err());
    }
}
