use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::engram::Status;
use crate::error::Result;
use crate::index::Index;
use crate::store::Library;

#[derive(Debug, Clone, Serialize)]
pub struct ThreadEntry {
    pub id: String,
    pub claim: String,
    pub last_touched: DateTime<Utc>,
    pub surface_when: Vec<String>,
    pub dormant_days: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ThreadsResult {
    pub threads: Vec<ThreadEntry>,
    pub surface_for: Option<String>,
}

/// List open threads (`unresolved_by_design`), optionally filtered by surfacing trigger.
pub fn list_threads(
    library: &Library,
    index: &Index,
    surface_for: Option<&str>,
) -> Result<ThreadsResult> {
    let now = Utc::now();
    let mut threads = Vec::new();

    if let Some(topic) = surface_for {
        let ids = index.engrams_matching_surface_trigger(topic)?;
        for id in ids {
            if let Some(entry) = thread_entry_from_id(library, index, &id, now)? {
                threads.push(entry);
            }
        }
    } else {
        let scan = library.scan_engrams();
        for engram in scan.engrams {
            if engram.status != Status::UnresolvedByDesign {
                continue;
            }
            let dormant_days = (now - engram.last_touched).num_seconds().max(0) as f64 / 86400.0;
            threads.push(ThreadEntry {
                id: engram.id,
                claim: engram.claim,
                last_touched: engram.last_touched,
                surface_when: engram.surface_when.unwrap_or_default(),
                dormant_days,
            });
        }
    }

    threads.sort_by(|a, b| b.last_touched.cmp(&a.last_touched));

    Ok(ThreadsResult {
        threads,
        surface_for: surface_for.map(str::to_string),
    })
}

fn thread_entry_from_id(
    _library: &Library,
    index: &Index,
    id: &str,
    now: DateTime<Utc>,
) -> Result<Option<ThreadEntry>> {
    let Some(path) = index.file_path(id)? else {
        return Ok(None);
    };
    let content = std::fs::read_to_string(&path)?;
    let engram = crate::engram::Engram::parse(&content)?;
    if engram.status != Status::UnresolvedByDesign {
        return Ok(None);
    }
    let dormant_days = (now - engram.last_touched).num_seconds().max(0) as f64 / 86400.0;
    Ok(Some(ThreadEntry {
        id: engram.id,
        claim: engram.claim,
        last_touched: engram.last_touched,
        surface_when: engram.surface_when.unwrap_or_default(),
        dormant_days,
    }))
}

/// Returns true if a trigger matches (supports `topic:foo` or plain substring).
pub fn trigger_matches(trigger: &str, topic: &str) -> bool {
    let topic_lower = topic.to_lowercase();
    let trigger_lower = trigger.to_lowercase();
    if let Some(rest) = trigger_lower.strip_prefix("topic:") {
        topic_lower.contains(rest) || rest.contains(&topic_lower)
    } else {
        trigger_lower.contains(&topic_lower) || topic_lower.contains(&trigger_lower)
    }
}
