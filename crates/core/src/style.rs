use serde::Serialize;

use crate::engram::{Engram, Tier};
use crate::error::Result;
use crate::provider::Completer;
use crate::store::Library;

/// Structured generation parameters — never quotable relational bodies (§2.4).
#[derive(Debug, Clone, Serialize, Default)]
pub struct StyleProfile {
    pub verbosity: f64,
    pub directness: f64,
    pub hedging: f64,
    pub pushback_tolerance: f64,
    pub pacing: String,
    pub evidence_summary: Option<RelationalEvidenceSummary>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct RelationalEvidenceSummary {
    pub projects: u32,
    pub task_types: u32,
    pub registers: u32,
}

/// Assemble style profile from relational engrams (salience-weighted heuristic).
pub fn style_profile(library: &Library, _completer: Option<&dyn Completer>) -> Result<StyleProfile> {
    let scan = library.scan_engrams();
    let relational: Vec<&Engram> = scan
        .engrams
        .iter()
        .filter(|e| e.tier == Tier::Relational)
        .collect();

    if relational.is_empty() {
        return Ok(StyleProfile::default());
    }

    let total_salience: f64 = relational.iter().map(|e| e.salience).sum();
    let weight = if total_salience > 0.0 {
        total_salience
    } else {
        relational.len() as f64
    };

    let mut verbosity = 0.0;
    let mut directness = 0.0;
    let mut hedging = 0.0;
    let mut pushback = 0.0;
    let mut pacing_score = 0.0;
    let mut evidence = RelationalEvidenceSummary::default();

    for e in &relational {
        let w = e.salience / weight;
        let cues = parse_relational_cues(e);
        verbosity += w * cues.verbosity;
        directness += w * cues.directness;
        hedging += w * cues.hedging;
        pushback += w * cues.pushback_tolerance;
        pacing_score += w * cues.pacing_numeric;
        if let Some(ev) = &cues.evidence {
            evidence.projects = evidence.projects.max(ev.projects);
            evidence.task_types = evidence.task_types.max(ev.task_types);
            evidence.registers = evidence.registers.max(ev.registers);
        }
    }

    let pacing = if pacing_score > 0.6 {
        "fast".into()
    } else if pacing_score < 0.4 {
        "deliberate".into()
    } else {
        "moderate".into()
    };

    Ok(StyleProfile {
        verbosity: verbosity.clamp(0.0, 1.0),
        directness: directness.clamp(0.0, 1.0),
        hedging: hedging.clamp(0.0, 1.0),
        pushback_tolerance: pushback.clamp(0.0, 1.0),
        pacing,
        evidence_summary: if evidence.projects + evidence.task_types + evidence.registers > 0 {
            Some(evidence)
        } else {
            None
        },
    })
}

struct RelationalCues {
    verbosity: f64,
    directness: f64,
    hedging: f64,
    pushback_tolerance: f64,
    pacing_numeric: f64,
    evidence: Option<RelationalEvidenceSummary>,
}

fn parse_relational_cues(engram: &Engram) -> RelationalCues {
    let text = format!("{} {}", engram.claim, engram.body).to_lowercase();
    let mut cues = RelationalCues {
        verbosity: 0.5,
        directness: 0.5,
        hedging: 0.5,
        pushback_tolerance: 0.5,
        pacing_numeric: 0.5,
        evidence: None,
    };

    if text.contains("terse") || text.contains("concise") || text.contains("brief") {
        cues.verbosity = 0.2;
    } else if text.contains("detailed") || text.contains("verbose") || text.contains("thorough") {
        cues.verbosity = 0.85;
    }

    if text.contains("direct") || text.contains("blunt") {
        cues.directness = 0.85;
    } else if text.contains("gentle") || text.contains("soft") {
        cues.directness = 0.25;
    }

    if text.contains("hedge") || text.contains("uncertain") {
        cues.hedging = 0.8;
    } else if text.contains("confident") {
        cues.hedging = 0.2;
    }

    if text.contains("pushback") || text.contains("challenge") || text.contains("disagree") {
        cues.pushback_tolerance = 0.85;
    } else if text.contains("defer") || text.contains("agreeable") {
        cues.pushback_tolerance = 0.2;
    }

    if text.contains("fast") || text.contains("quick") || text.contains("pacing:fast") {
        cues.pacing_numeric = 0.8;
    } else if text.contains("slow") || text.contains("deliberate") {
        cues.pacing_numeric = 0.2;
    }

    for tag in &engram.tags {
        if let Some(ev) = parse_evidence_tag(tag) {
            cues.evidence = Some(ev);
        }
    }

    cues
}

fn parse_evidence_tag(tag: &str) -> Option<RelationalEvidenceSummary> {
    if let Some(rest) = tag.strip_prefix("evidence:") {
        let mut ev = RelationalEvidenceSummary::default();
        for part in rest.split(',') {
            let part = part.trim();
            if let Some(n) = part.strip_prefix("projects=").and_then(|s| s.parse().ok()) {
                ev.projects = n;
            } else if let Some(n) = part.strip_prefix("task_types=").and_then(|s| s.parse().ok()) {
                ev.task_types = n;
            } else if let Some(n) = part.strip_prefix("registers=").and_then(|s| s.parse().ok()) {
                ev.registers = n;
            }
        }
        return Some(ev);
    }
    None
}
