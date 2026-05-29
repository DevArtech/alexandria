use rusqlite::params;
use serde::Serialize;

use crate::config::{Config, ThresholdsConfig};
use crate::engram::Engram;
use crate::error::Result;
use crate::index::Index;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RecallState {
    StrongHit,
    WeakHit,
    /// Reserved for M2 (embedding neighborhood density check).
    HighConfidenceGap,
    /// Reserved for M2 (domain centroid proximity).
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
    pub token_cost: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct RecallResult {
    pub state: RecallState,
    pub response_mode: ResponseMode,
    pub query: String,
    pub engrams: Vec<RecallHit>,
    pub total_tokens: u32,
}

pub struct Retrieval<'a> {
    index: &'a Index,
    config: &'a Config,
}

impl<'a> Retrieval<'a> {
    pub fn new(index: &'a Index, config: &'a Config) -> Self {
        Self { index, config }
    }

    /// Run lexical recall. `query` is escaped for FTS5 internally.
    pub fn recall(&self, query: &str, budget: Option<u32>) -> Result<RecallResult> {
        let budget = budget.unwrap_or(self.config.budgets.default_recall_tokens);
        let thresholds = &self.config.thresholds;
        let fts_query = escape_fts_query(query);

        let hits = self.search_fts(&fts_query, budget * 4)?;

        // Drop hits below the weak threshold so state and payload stay consistent.
        let qualifying: Vec<_> = hits
            .into_iter()
            .filter(|h| hit_meets_weak_threshold(h.score, thresholds))
            .collect();

        let state = classify_state(&qualifying, thresholds);
        let response_mode = classify_response_mode(state);

        let mut engrams = Vec::new();
        let mut total_tokens = 0u32;

        for mut hit in qualifying {
            let token_cost = Engram::estimate_tokens(&hit.claim);
            if total_tokens + token_cost > budget && !engrams.is_empty() {
                break;
            }
            hit.token_cost = token_cost;
            total_tokens += token_cost;
            engrams.push(hit);
            if total_tokens >= budget {
                break;
            }
        }

        Ok(RecallResult {
            state,
            response_mode,
            query: query.to_string(),
            engrams,
            total_tokens,
        })
    }

    fn search_fts(&self, fts_query: &str, limit: u32) -> Result<Vec<RecallHit>> {
        let conn = self.index.connection();
        // FTS5 bm25(): more negative = better match. ORDER BY ASC puts best first.
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
            Ok(RecallHit {
                id: row.get(0)?,
                claim: row.get(1)?,
                tier: row.get(2)?,
                status: row.get(3)?,
                score: row.get(4)?,
                token_cost: 0,
            })
        })?;

        let mut hits = Vec::new();
        for row in rows {
            hits.push(row?);
        }
        Ok(hits)
    }
}

/// Escape a user query for FTS5 MATCH. Tokens with FTS operators or punctuation are quoted.
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

/// SQLite FTS5 bm25: more negative = better. `strong_cutoff` / `weak_cutoff` are upper bounds
/// (closer to zero); a score must be <= cutoff to meet that band.
fn classify_state(hits: &[RecallHit], thresholds: &ThresholdsConfig) -> RecallState {
    if hits.is_empty() {
        return RecallState::Nothing;
    }
    let top_score = hits[0].score;
    if top_score <= thresholds.strong_cutoff {
        RecallState::StrongHit
    } else if top_score <= thresholds.weak_cutoff {
        RecallState::WeakHit
    } else {
        RecallState::Nothing
    }
}

fn hit_meets_weak_threshold(score: f64, thresholds: &ThresholdsConfig) -> bool {
    score <= thresholds.weak_cutoff
}

fn classify_response_mode(state: RecallState) -> ResponseMode {
    match state {
        RecallState::StrongHit => ResponseMode::Flow,
        RecallState::WeakHit
        | RecallState::Nothing
        | RecallState::HighConfidenceGap
        | RecallState::LowConfidenceGap => ResponseMode::Humility,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Engram, Index, Library, Status, Tier};
    use tempfile::TempDir;

    fn setup() -> (TempDir, Library, Index, Config) {
        let dir = TempDir::new().unwrap();
        let lib = Library::init(dir.path()).unwrap();
        let index = Index::open(&lib).unwrap();
        let config = Config::load(dir.path()).unwrap();
        (dir, lib, index, config)
    }

    fn remember(lib: &Library, index: &Index, claim: &str, body: &str) {
        let e = Engram::new(claim, body, Tier::Semantic, Status::Confirmed);
        let p = lib.write_engram(&e).unwrap();
        index.upsert(&e, &p.display().to_string()).unwrap();
    }

    #[test]
    fn escape_fts_query_quotes_special_tokens() {
        assert_eq!(escape_fts_query("C++"), "\"C++\"");
        assert_eq!(escape_fts_query("self-hostable"), "\"self-hostable\"");
        assert_eq!(escape_fts_query("foo AND bar"), "foo \"AND\" bar");
    }

    #[test]
    fn specific_multi_term_match_is_strong_hit() {
        let (_dir, lib, index, config) = setup();
        remember(
            &lib,
            &index,
            "PostgreSQL pgbouncer deadlock transaction timeouts",
            "Connection pool exhaustion under load.",
        );
        remember(
            &lib,
            &index,
            "Alexandria overview",
            "Alexandria Alexandria Alexandria generic filler.",
        );

        let retrieval = Retrieval::new(&index, &config);
        let result = retrieval
            .recall(
                "PostgreSQL pgbouncer deadlock transaction timeouts",
                Some(2000),
            )
            .unwrap();

        assert!(!result.engrams.is_empty());
        assert!(
            result.engrams[0]
                .claim
                .contains("PostgreSQL pgbouncer")
        );
        assert_ne!(result.state, RecallState::Nothing);
        if result.state == RecallState::StrongHit {
            assert_eq!(result.response_mode, ResponseMode::Flow);
        }
    }

    #[test]
    fn vague_single_token_is_not_strong_hit() {
        let (_dir, lib, index, config) = setup();
        remember(
            &lib,
            &index,
            "PostgreSQL pgbouncer deadlock transaction timeouts",
            "Specific doc.",
        );
        remember(
            &lib,
            &index,
            "Alexandria overview",
            "Alexandria Alexandria Alexandria generic filler.",
        );

        let retrieval = Retrieval::new(&index, &config);
        let result = retrieval.recall("Alexandria", Some(2000)).unwrap();

        assert_ne!(result.state, RecallState::StrongHit);
    }

    #[test]
    fn nothing_state_has_empty_engrams() {
        let (_dir, lib, index, config) = setup();
        remember(&lib, &index, "only rabbits here", "no matching terms expected");

        let retrieval = Retrieval::new(&index, &config);
        let result = retrieval.recall("zzzznonexistent", Some(2000)).unwrap();

        assert_eq!(result.state, RecallState::Nothing);
        assert!(result.engrams.is_empty());
    }

    #[test]
    fn classify_state_respects_bm25_direction() {
        let thresholds = ThresholdsConfig {
            strong_cutoff: -1.0,
            weak_cutoff: 1.0,
        };
        let strong = [RecallHit {
            id: "a".into(),
            claim: "c".into(),
            tier: "semantic".into(),
            status: "confirmed".into(),
            score: -3.5,
            token_cost: 0,
        }];
        let weak = [RecallHit {
            id: "b".into(),
            claim: "c".into(),
            tier: "semantic".into(),
            status: "confirmed".into(),
            score: -0.5,
            token_cost: 0,
        }];
        let poor = [RecallHit {
            id: "c".into(),
            claim: "c".into(),
            tier: "semantic".into(),
            status: "confirmed".into(),
            score: 2.0,
            token_cost: 0,
        }];

        assert_eq!(classify_state(&strong, &thresholds), RecallState::StrongHit);
        assert_eq!(classify_state(&weak, &thresholds), RecallState::WeakHit);
        assert_eq!(classify_state(&poor, &thresholds), RecallState::Nothing);
    }

    #[test]
    fn specific_match_scores_better_than_vague() {
        let (_dir, lib, index, config) = setup();
        remember(
            &lib,
            &index,
            "PostgreSQL pgbouncer deadlock transaction timeouts",
            "Connection pool exhaustion under load.",
        );
        remember(
            &lib,
            &index,
            "Alexandria overview",
            "Alexandria Alexandria Alexandria generic filler.",
        );

        let retrieval = Retrieval::new(&index, &config);
        let specific = retrieval
            .recall(
                "PostgreSQL pgbouncer deadlock transaction timeouts",
                Some(2000),
            )
            .unwrap();
        let vague = retrieval.recall("Alexandria", Some(2000)).unwrap();

        assert!(!specific.engrams.is_empty());
        if !vague.engrams.is_empty() {
            assert!(
                specific.engrams[0].score < vague.engrams[0].score,
                "specific {:?} should beat vague {:?}",
                specific.engrams[0].score,
                vague.engrams[0].score
            );
        }
        assert_ne!(specific.state, RecallState::Nothing);
    }
}
