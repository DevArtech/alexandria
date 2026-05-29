//! Memory-density diagnostics for a topic: an x-ray of how much the library knows.

use serde::Serialize;

use crate::config::Config;
use crate::error::Result;
use crate::facets::{detect_facets, dominant_facet, facets_to_filters, DetectedFacet};
use crate::index::Index;
use crate::retrieval::escape_fts_query;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RecommendedNext {
    Nothing,
    Recall,
    Expand,
    Survey,
}

impl RecommendedNext {
    pub fn as_str(self) -> &'static str {
        match self {
            RecommendedNext::Nothing => "nothing",
            RecommendedNext::Recall => "recall",
            RecommendedNext::Expand => "expand",
            RecommendedNext::Survey => "survey",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct RecencyRange {
    pub oldest_updated: Option<String>,
    pub newest_updated: Option<String>,
    pub oldest_touched: Option<String>,
    pub newest_touched: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProvenanceDepth {
    pub total_sources: u32,
    pub first_party_sources: u32,
    pub derived_sources: u32,
    pub engrams_with_sources: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct CoverageReport {
    pub topic: String,
    pub detected_facets: Vec<DetectedFacet>,
    pub engram_count: u32,
    pub canonical_count: u32,
    pub provisional_count: u32,
    pub open_threads: u32,
    pub provenance: ProvenanceDepth,
    pub recency: RecencyRange,
    pub density_neighbors: u32,
    pub claim_tokens: u32,
    pub body_tokens: u32,
    pub detail_ratio: f64,
    pub detail_sparse: bool,
    pub meta_reliability: f64,
    pub recommended_next: RecommendedNext,
}

/// Build a memory-density x-ray for a topic.
pub fn coverage(topic: &str, index: &Index, config: &Config) -> Result<CoverageReport> {
    let detected_facets = detect_facets(index, topic)?;
    let (collections, tags) = facets_to_filters(&detected_facets);

    let mut ids = index.engram_ids_matching(&collections, &tags)?;
    let fts_ids = index.fts_engram_ids(&escape_fts_query(topic), 200)?;
    for id in fts_ids {
        if !ids.contains(&id) {
            ids.push(id);
        }
    }

    ids.retain(|id| {
        index
            .get_engram(id)
            .ok()
            .flatten()
            .is_some_and(|row| row.tier != crate::engram::Tier::Relational)
    });

    let engram_count = ids.len() as u32;
    let status_counts = index.status_counts_for_ids(&ids)?;
    let canonical_count = status_counts.confirmed;
    let provisional_count = status_counts.provisional;
    let open_threads = status_counts.unresolved_by_design;

    let prov_stats = index.provenance_stats_for_ids(&ids)?;
    let provenance = ProvenanceDepth {
        total_sources: prov_stats.total_sources,
        first_party_sources: prov_stats.first_party_sources,
        derived_sources: prov_stats.derived_sources,
        engrams_with_sources: prov_stats.engrams_with_sources,
    };
    let recency_stats = index.recency_stats_for_ids(&ids)?;
    let recency = RecencyRange {
        oldest_updated: recency_stats.oldest_updated,
        newest_updated: recency_stats.newest_updated,
        oldest_touched: recency_stats.oldest_touched,
        newest_touched: recency_stats.newest_touched,
    };
    let (claim_tokens, body_tokens) = index.token_costs_for_ids(&ids)?;
    let detail_ratio = if claim_tokens > 0 {
        body_tokens as f64 / claim_tokens as f64
    } else {
        0.0
    };
    let detail_sparse = engram_count > 0 && detail_ratio < 2.0;

    let query_vec = index.embed_query(topic)?;
    let density_neighbors = index.neighbors_within(&query_vec, config.thresholds.density_radius)?;

    let domain = dominant_facet(&detected_facets)
        .map(|f| f.name.as_str())
        .or(collections.first().map(String::as_str));
    let meta_reliability = index.meta_reliability(domain)?;

    let recommended_next = recommend_next(
        engram_count,
        detail_sparse,
        density_neighbors,
        config.thresholds.density_min_count,
    );

    Ok(CoverageReport {
        topic: topic.to_string(),
        detected_facets,
        engram_count,
        canonical_count,
        provisional_count,
        open_threads,
        provenance,
        recency,
        density_neighbors,
        claim_tokens,
        body_tokens,
        detail_ratio,
        detail_sparse,
        meta_reliability,
        recommended_next,
    })
}

fn recommend_next(
    engram_count: u32,
    detail_sparse: bool,
    density_neighbors: u32,
    density_min_count: u32,
) -> RecommendedNext {
    if engram_count == 0 {
        if density_neighbors >= density_min_count {
            return RecommendedNext::Recall;
        }
        return RecommendedNext::Nothing;
    }
    if detail_sparse && engram_count <= 3 {
        return RecommendedNext::Expand;
    }
    if engram_count >= 5 || detail_sparse {
        return RecommendedNext::Survey;
    }
    RecommendedNext::Expand
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recommend_nothing_when_empty_and_sparse() {
        assert_eq!(
            recommend_next(0, false, 0, 3),
            RecommendedNext::Nothing
        );
    }

    #[test]
    fn recommend_survey_when_many_engrams() {
        assert_eq!(
            recommend_next(5, false, 10, 3),
            RecommendedNext::Survey
        );
    }

    #[test]
    fn recommend_expand_when_sparse_and_few() {
        assert_eq!(
            recommend_next(2, true, 10, 3),
            RecommendedNext::Expand
        );
    }
}
