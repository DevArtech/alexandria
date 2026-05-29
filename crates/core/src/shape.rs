use crate::engram::Engram;
use crate::provider::{Completer, Prompt};

/// Heuristic structural shape summary for an episodic engram (ARCHITECTURE §6.3.1).
pub fn extract_shape_summary_heuristic(engram: &Engram) -> String {
    let body = &engram.body;
    let problem = extract_section(body, &["problem", "issue", "symptom", "error"]);
    let hypotheses = extract_section(body, &["hypothesis", "tried", "attempt", "approach"]);
    let dead_ends = extract_section(body, &["dead end", "failed", "didn't work", "blocked"]);
    let resolution = extract_section(body, &["resolved", "solution", "fixed", "outcome"]);

    let mut parts = vec![format!("Problem arc for: {}", engram.claim)];
    if let Some(p) = problem {
        parts.push(format!("Problem: {p}"));
    }
    if let Some(h) = hypotheses {
        parts.push(format!("Hypotheses tried: {h}"));
    }
    if let Some(d) = dead_ends {
        parts.push(format!("Dead ends: {d}"));
    }
    if let Some(r) = resolution {
        parts.push(format!("Resolution: {r}"));
    }
    if parts.len() == 1 {
        let digest: String = body
            .lines()
            .filter(|l| !l.trim().is_empty())
            .take(8)
            .collect::<Vec<_>>()
            .join("; ");
        if digest.is_empty() {
            parts.push(format!("Structural digest: {}", engram.claim));
        } else {
            parts.push(format!("Structural digest: {digest}"));
        }
    }
    parts.join("\n")
}

/// Shape summary with optional Completer hook; falls back to heuristic when None.
pub fn extract_shape_summary(
    engram: &Engram,
    completer: Option<&dyn Completer>,
) -> crate::error::Result<String> {
    if let Some(c) = completer {
        let prompt = Prompt {
            system: Some(
                "Extract a structural problem-arc summary: problem, hypotheses tried, dead ends, resolution. Be concise.".into(),
            ),
            user: format!("Claim: {}\n\nBody:\n{}", engram.claim, engram.body),
        };
        if let Ok(summary) = c.complete(&prompt) {
            if !summary.trim().is_empty() {
                return Ok(summary);
            }
        }
    }
    Ok(extract_shape_summary_heuristic(engram))
}

fn extract_section(body: &str, keywords: &[&str]) -> Option<String> {
    for line in body.lines() {
        let lower = line.to_lowercase();
        if keywords.iter().any(|k| lower.contains(k)) {
            let trimmed = line.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.chars().take(200).collect());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engram::{Engram, Status, Tier};

    #[test]
    fn heuristic_includes_claim() {
        let e = Engram::new(
            "debugging pool exhaustion",
            "Problem: connections maxed out.\nTried: increase pool size.\nFailed: still OOM.\nResolved: add pgbouncer.",
            Tier::Episodic,
            Status::Confirmed,
        );
        let summary = extract_shape_summary_heuristic(&e);
        assert!(summary.contains("debugging pool"));
        assert!(summary.contains("Problem") || summary.contains("Resolved"));
    }
}
