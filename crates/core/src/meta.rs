use std::fs::{self, OpenOptions};
use std::io::Write;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::index::Index;
use crate::store::Library;

const META_LOG_DIR: &str = "meta_log";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MetaLogEvent {
    Correction {
        domain: String,
        engram_id: Option<String>,
        timestamp: DateTime<Utc>,
    },
    GapOutcome {
        domain: String,
        gap_kind: String,
        false_positive: bool,
        timestamp: DateTime<Utc>,
    },
    PromotionReversal {
        engram_id: String,
        from_tier: String,
        timestamp: DateTime<Utc>,
    },
}

#[derive(Debug, Clone, Serialize)]
pub struct MetaReport {
    pub domain: Option<String>,
    pub reliability: f64,
    pub recent_corrections: u32,
    pub gap_false_positive_rate: f64,
    pub promotion_reversal_rate: f64,
    pub total_corrections: u32,
    pub total_gaps: u32,
    pub total_reversals: u32,
}

pub fn meta_log_dir(library: &Library) -> std::path::PathBuf {
    library.alexandria_dir().join(META_LOG_DIR)
}

/// Append an event to the meta log (survives reindex).
pub fn append_meta_event(library: &Library, event: &MetaLogEvent) -> Result<()> {
    let dir = meta_log_dir(library);
    fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{}.jsonl", Utc::now().format("%Y-%m")));
    let line = serde_json::to_string(event)
        .map_err(|e| crate::error::AlexandriaError::Other(anyhow::anyhow!("{e}")))?;
    let mut file = OpenOptions::new().create(true).append(true).open(&path)?;
    writeln!(file, "{line}")?;
    Ok(())
}

/// Record a user correction in domain.
pub fn record_correction(library: &Library, domain: &str, engram_id: Option<&str>) -> Result<()> {
    append_meta_event(
        library,
        &MetaLogEvent::Correction {
            domain: domain.to_string(),
            engram_id: engram_id.map(str::to_string),
            timestamp: Utc::now(),
        },
    )
}

/// Record gap outcome (false positive when no relevant memory existed).
pub fn record_gap_outcome(
    library: &Library,
    domain: &str,
    gap_kind: &str,
    false_positive: bool,
) -> Result<()> {
    append_meta_event(
        library,
        &MetaLogEvent::GapOutcome {
            domain: domain.to_string(),
            gap_kind: gap_kind.to_string(),
            false_positive,
            timestamp: Utc::now(),
        },
    )
}

/// Record promotion reversal on demotion.
pub fn record_promotion_reversal(
    library: &Library,
    engram_id: &str,
    from_tier: &str,
) -> Result<()> {
    append_meta_event(
        library,
        &MetaLogEvent::PromotionReversal {
            engram_id: engram_id.to_string(),
            from_tier: from_tier.to_string(),
            timestamp: Utc::now(),
        },
    )
}

/// Load all events from meta_log directory.
pub fn load_meta_events(library: &Library) -> Result<Vec<MetaLogEvent>> {
    let dir = meta_log_dir(library);
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut events = Vec::new();
    for entry in fs::read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("jsonl") {
            continue;
        }
        let content = fs::read_to_string(&path)?;
        for line in content.lines() {
            if line.trim().is_empty() {
                continue;
            }
            if let Ok(ev) = serde_json::from_str::<MetaLogEvent>(line) {
                events.push(ev);
            }
        }
    }
    Ok(events)
}

/// Rebuild meta-memory index tables from append-only log.
pub fn rebuild_meta_index(index: &Index, library: &Library) -> Result<()> {
    let events = load_meta_events(library)?;
    index.clear_meta_tables()?;
    for event in &events {
        match event {
            MetaLogEvent::Correction {
                domain,
                engram_id,
                timestamp,
            } => {
                index.insert_correction(domain, engram_id.as_deref(), timestamp)?;
            }
            MetaLogEvent::GapOutcome {
                domain,
                gap_kind,
                false_positive,
                timestamp,
            } => {
                index.insert_gap_outcome(domain, gap_kind, *false_positive, timestamp)?;
            }
            MetaLogEvent::PromotionReversal {
                engram_id,
                from_tier,
                timestamp,
            } => {
                index.insert_promotion_reversal(engram_id, from_tier, timestamp)?;
            }
        }
    }
    index.recompute_meta_reliability()?;
    Ok(())
}

/// Aggregate meta report for optional domain filter.
pub fn meta_report(_library: &Library, index: &Index, domain: Option<&str>) -> Result<MetaReport> {
    let reliability = index.meta_reliability(domain)?;
    let recent_corrections = index.recent_corrections_count(domain, 30)?;
    let (gap_fp_rate, total_gaps) = index.gap_false_positive_rate(domain)?;
    let (reversal_rate, total_reversals) = index.promotion_reversal_rate(domain)?;
    let total_corrections = index.total_corrections(domain)?;

    Ok(MetaReport {
        domain: domain.map(str::to_string),
        reliability,
        recent_corrections,
        gap_false_positive_rate: gap_fp_rate,
        promotion_reversal_rate: reversal_rate,
        total_corrections,
        total_gaps,
        total_reversals,
    })
}

/// Domain reliability below this threshold forces humility in posture judge.
/// Default posture/calibration cutoff. Must be **above** the correction-only reliability
/// floor (0.5) so domain corrections can trigger humility without requiring gap FPs.
pub const DEFAULT_RELIABILITY_THRESHOLD: f64 = 0.6;

pub fn domain_reliability_weak(index: &Index, domain: Option<&str>) -> Result<bool> {
    let reliability = index.meta_reliability(domain)?;
    Ok(reliability < DEFAULT_RELIABILITY_THRESHOLD)
}

/// Infer domain from first collection on a query's top hit collections.
pub fn domain_from_collections(collections: &[String]) -> Option<String> {
    collections.first().cloned()
}
