//! Source freshness: staleness signals from optional per-source observation times.

use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::config::FreshnessConfig;
use crate::index::{Index, SourceFreshness};

#[derive(Debug, Clone, Serialize)]
pub struct FreshnessHint {
    pub youngest_observed: Option<String>,
    pub age_days: Option<u32>,
    pub warning: Option<String>,
}

/// Build a freshness hint for an engram; returns None when observation time is unknown.
pub fn freshness_hint(
    index: &Index,
    engram_id: &str,
    config: &FreshnessConfig,
) -> crate::error::Result<Option<FreshnessHint>> {
    if !config.enabled {
        return Ok(None);
    }
    let Some(youngest) = index.max_source_observed(engram_id)? else {
        return Ok(None);
    };
    let age_days = age_days_since(youngest);
    let warning = if age_days >= config.stale_after_days {
        Some(format!(
            "youngest source observed {} ({}d ago); may be stale",
            youngest.to_rfc3339(),
            age_days
        ))
    } else {
        None
    };
    Ok(Some(FreshnessHint {
        youngest_observed: Some(youngest.to_rfc3339()),
        age_days: Some(age_days),
        warning,
    }))
}

/// Annotate source rows with age in days when observed is known.
pub fn annotate_sources(
    sources: Vec<crate::engram::Source>,
) -> Vec<SourceFreshness> {
    sources
        .into_iter()
        .map(|s| {
            let age_days = s.observed.map(age_days_since);
            SourceFreshness {
                kind: s.kind,
                reference: s.r#ref,
                observed: s.observed.map(|t| t.to_rfc3339()),
                age_days,
            }
        })
        .collect()
}

pub fn age_days_since(when: DateTime<Utc>) -> u32 {
    let now = Utc::now();
    let duration = now.signed_duration_since(when);
    duration.num_days().max(0) as u32
}
