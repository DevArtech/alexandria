use rusqlite::params;
use serde::Serialize;

use crate::engram::{Engram, Rel, Tier};
use crate::error::{AlexandriaError, Result};
use crate::index::Index;

#[derive(Debug, Clone, Serialize)]
pub struct TraceNode {
    pub id: String,
    pub claim: String,
    pub tier: String,
    pub status: String,
    pub confidence: f64,
    pub source_kind: String,
    pub source_ref: String,
    pub depth: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct TraceResult {
    pub id: String,
    pub claim: String,
    pub confidence: f64,
    pub effective_confidence: f64,
    pub has_derived_sources: bool,
    pub nodes: Vec<TraceNode>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TimelineEntry {
    pub id: String,
    pub claim: String,
    pub tier: String,
    pub status: String,
    pub created: String,
    pub last_touched: String,
    pub confidence: f64,
    pub salience: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct TimelineResult {
    pub entries: Vec<TimelineEntry>,
    pub count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct TraverseNode {
    pub id: String,
    pub claim: String,
    pub tier: String,
    pub status: String,
    pub rel: String,
    pub depth: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct TraverseResult {
    pub from_id: String,
    pub nodes: Vec<TraverseNode>,
}

pub struct Graph<'a> {
    index: &'a Index,
}

impl<'a> Graph<'a> {
    pub fn new(index: &'a Index) -> Self {
        Self { index }
    }

    /// Walk the provenance DAG following derived sources back to first-party leaves.
    pub fn trace(&self, id: &str) -> Result<TraceResult> {
        let conn = self.index.connection();
        let root = conn.query_row(
            "SELECT id, claim, confidence FROM engrams WHERE id = ?1",
            params![id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, f64>(2)?,
                ))
            },
        );
        let (root_id, root_claim, root_confidence) = match root {
            Ok(r) => r,
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                return Err(AlexandriaError::EngramNotFound(id.to_string()));
            }
            Err(e) => return Err(e.into()),
        };

        let mut stmt = conn.prepare(
            r#"
            WITH RECURSIVE provenance(id, claim, tier, status, confidence, source_kind, source_ref, depth) AS (
              SELECT e.id, e.claim, e.tier, e.status, e.confidence, s.kind, s.ref, 0
              FROM engrams e
              JOIN sources s ON s.engram_id = e.id
              WHERE e.id = ?1
              UNION ALL
              SELECT e.id, e.claim, e.tier, e.status, e.confidence, s.kind, s.ref, p.depth + 1
              FROM provenance p
              JOIN engrams e ON e.id = p.source_ref
              JOIN sources s ON s.engram_id = e.id
              WHERE p.source_kind = 'derived' AND p.depth < 20
            )
            SELECT id, claim, tier, status, confidence, source_kind, source_ref, depth
            FROM provenance
            ORDER BY depth, id
            "#,
        )?;

        let rows = stmt.query_map(params![id], |row| {
            Ok(TraceNode {
                id: row.get(0)?,
                claim: row.get(1)?,
                tier: row.get(2)?,
                status: row.get(3)?,
                confidence: row.get(4)?,
                source_kind: row.get(5)?,
                source_ref: row.get(6)?,
                depth: row.get(7)?,
            })
        })?;

        let mut nodes = Vec::new();
        for row in rows {
            nodes.push(row?);
        }

        let has_derived_sources = nodes.iter().any(|n| n.source_kind == "derived");
        let premise_confidences: Vec<f64> = nodes
            .iter()
            .filter(|n| n.depth > 0)
            .map(|n| n.confidence)
            .collect();
        let effective_confidence = compute_effective_confidence_raw(
            self.index,
            root_confidence,
            id,
            &premise_confidences,
        )?;

        Ok(TraceResult {
            id: root_id,
            claim: root_claim,
            confidence: root_confidence,
            effective_confidence,
            has_derived_sources,
            nodes,
        })
    }

    /// Episodic view over time, optionally filtered by tier and date bounds.
    pub fn timeline(
        &self,
        since: Option<&str>,
        until: Option<&str>,
        tier: Option<Tier>,
    ) -> Result<TimelineResult> {
        let conn = self.index.connection();
        let mut sql = String::from(
            "SELECT id, claim, tier, status, created, last_touched, confidence, salience \
             FROM engrams WHERE 1=1",
        );
        let mut bind: Vec<String> = Vec::new();

        if let Some(t) = tier {
            sql.push_str(" AND tier = ?");
            bind.push(tier_str(t).to_string());
        }
        if let Some(s) = since {
            sql.push_str(" AND created >= ?");
            bind.push(s.to_string());
        }
        if let Some(u) = until {
            sql.push_str(" AND created <= ?");
            bind.push(u.to_string());
        }
        sql.push_str(" ORDER BY created ASC, last_touched ASC");

        let mut stmt = conn.prepare(&sql)?;
        let rows = match bind.len() {
            0 => stmt.query_map([], map_timeline_row)?,
            1 => stmt.query_map(params![bind[0]], map_timeline_row)?,
            2 => stmt.query_map(params![bind[0], bind[1]], map_timeline_row)?,
            3 => stmt.query_map(params![bind[0], bind[1], bind[2]], map_timeline_row)?,
            _ => {
                return Err(AlexandriaError::Other(anyhow::anyhow!(
                    "unexpected bind count"
                )));
            }
        };

        let mut entries = Vec::new();
        for row in rows {
            entries.push(row?);
        }
        let count = entries.len();
        Ok(TimelineResult { entries, count })
    }

    /// Multi-hop typed-edge traversal from a starting engram.
    pub fn traverse(
        &self,
        from_id: &str,
        rels: Option<&[Rel]>,
        max_depth: u32,
    ) -> Result<TraverseResult> {
        let conn = self.index.connection();
        let exists: i64 = conn.query_row(
            "SELECT COUNT(*) FROM engrams WHERE id = ?1",
            params![from_id],
            |row| row.get(0),
        )?;
        if exists == 0 {
            return Err(AlexandriaError::EngramNotFound(from_id.to_string()));
        }

        let depth = max_depth.clamp(1, 10);
        let rel_filter = rels.map(|r| {
            r.iter()
                .map(|rel| format!("'{}'", rel.as_str()))
                .collect::<Vec<_>>()
                .join(", ")
        });

        let sql = if let Some(filter) = rel_filter {
            format!(
                r#"
                WITH RECURSIVE walk(from_id, to_id, rel, depth) AS (
                  SELECT from_id, to_id, rel, 1
                  FROM edges
                  WHERE from_id = ?1 AND rel IN ({filter})
                  UNION ALL
                  SELECT e.from_id, e.to_id, e.rel, w.depth + 1
                  FROM edges e
                  JOIN walk w ON e.from_id = w.to_id
                  WHERE w.depth < ?2 AND e.rel IN ({filter})
                )
                SELECT w.to_id, en.claim, en.tier, en.status, w.rel, w.depth
                FROM walk w
                JOIN engrams en ON en.id = w.to_id
                ORDER BY w.depth, w.rel, w.to_id
                "#
            )
        } else {
            r#"
            WITH RECURSIVE walk(from_id, to_id, rel, depth) AS (
              SELECT from_id, to_id, rel, 1
              FROM edges
              WHERE from_id = ?1
              UNION ALL
              SELECT e.from_id, e.to_id, e.rel, w.depth + 1
              FROM edges e
              JOIN walk w ON e.from_id = w.to_id
              WHERE w.depth < ?2
            )
            SELECT w.to_id, en.claim, en.tier, en.status, w.rel, w.depth
            FROM walk w
            JOIN engrams en ON en.id = w.to_id
            ORDER BY w.depth, w.rel, w.to_id
            "#
            .to_string()
        };

        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params![from_id, depth], |row| {
            Ok(TraverseNode {
                id: row.get(0)?,
                claim: row.get(1)?,
                tier: row.get(2)?,
                status: row.get(3)?,
                rel: row.get(4)?,
                depth: row.get(5)?,
            })
        })?;

        let mut nodes = Vec::new();
        for row in rows {
            nodes.push(row?);
        }

        Ok(TraverseResult {
            from_id: from_id.to_string(),
            nodes,
        })
    }
}

fn map_timeline_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<TimelineEntry> {
    Ok(TimelineEntry {
        id: row.get(0)?,
        claim: row.get(1)?,
        tier: row.get(2)?,
        status: row.get(3)?,
        created: row.get(4)?,
        last_touched: row.get(5)?,
        confidence: row.get(6)?,
        salience: row.get(7)?,
    })
}

fn tier_str(tier: Tier) -> &'static str {
    match tier {
        Tier::Working => "working",
        Tier::Episodic => "episodic",
        Tier::Provisional => "provisional",
        Tier::Semantic => "semantic",
        Tier::Procedural => "procedural",
        Tier::Relational => "relational",
    }
}

/// Compute effective confidence after conflict penalty and derived-source bounds.
pub fn effective_confidence(
    confidence: f64,
    has_unresolved_conflict: bool,
    premise_confidences: &[f64],
) -> f64 {
    let mut effective = confidence;
    if has_unresolved_conflict {
        effective *= 0.5;
    }
    if !premise_confidences.is_empty() {
        effective = effective.min(
            premise_confidences
                .iter()
                .copied()
                .fold(f64::INFINITY, f64::min),
        );
    }
    effective
}

/// Canonical effective-confidence computation for an engram (conflicts + direct derived premises).
pub fn compute_effective_confidence(index: &Index, engram: &Engram) -> Result<f64> {
    let mut premises = Vec::new();
    for source in &engram.source {
        if source.kind == "derived" {
            if let Ok(Some(row)) = index.get_engram(&source.r#ref) {
                premises.push(row.confidence);
            }
        }
    }
    compute_effective_confidence_raw(index, engram.confidence, &engram.id, &premises)
}

/// Effective confidence from raw fields (used by trace with multi-hop premise list).
pub fn compute_effective_confidence_raw(
    index: &Index,
    confidence: f64,
    engram_id: &str,
    premise_confidences: &[f64],
) -> Result<f64> {
    Ok(effective_confidence(
        confidence,
        has_conflicts_confirmed(index, engram_id)?,
        premise_confidences,
    ))
}

/// Returns true when the engram has an unresolved outgoing conflicts_confirmed edge.
pub fn has_conflicts_confirmed(index: &Index, id: &str) -> Result<bool> {
    let conn = index.connection();
    let count: i64 = conn.query_row(
        r#"
        SELECT COUNT(*) FROM edges e
        JOIN engrams t ON t.id = e.to_id
        WHERE e.from_id = ?1
          AND e.rel = 'conflicts_confirmed'
          AND t.status NOT IN ('superseded', 'archived')
        "#,
        params![id],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

/// Count incoming supports edges (corroboration signal).
pub fn incoming_supports_count(index: &Index, id: &str) -> Result<u32> {
    let conn = index.connection();
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM edges WHERE to_id = ?1 AND rel = 'supports'",
        params![id],
        |row| row.get(0),
    )?;
    Ok(count as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engram::{Engram, Link, Source, Status, Tier};
    use crate::provider::build_embedder;
    use crate::{Config, Index, Library};
    use chrono::Utc;
    use tempfile::TempDir;

    fn setup() -> (TempDir, Library, Index, Config) {
        let dir = TempDir::new().unwrap();
        let lib = Library::init(dir.path()).unwrap();
        let mut config = Config::load(dir.path()).unwrap();
        config.providers.embedder = "hash".into();
        let embedder = build_embedder(&config).unwrap();
        let index = Index::open_with_embedder(&lib, embedder).unwrap();
        (dir, lib, index, config)
    }

    fn remember(lib: &Library, index: &Index, engram: &Engram) {
        let p = lib.write_engram(engram).unwrap();
        index.upsert(engram, &p.display().to_string()).unwrap();
    }

    #[test]
    fn trace_applies_conflict_penalty() {
        let (_dir, lib, index, _config) = setup();
        let a = Engram::new("conflicted claim", "b", Tier::Semantic, Status::Confirmed);
        let b = Engram::new("other claim", "b", Tier::Semantic, Status::Confirmed);
        remember(&lib, &index, &b);
        remember(&lib, &index, &a);

        let ops = crate::ops::Ops::new(&lib, &index);
        ops.link(&a.id, Rel::ConflictsConfirmed, &b.id).unwrap();

        let graph = Graph::new(&index);
        let result = graph.trace(&a.id).unwrap();
        assert!((result.effective_confidence - 0.45).abs() < f64::EPSILON);
    }

    #[test]
    fn conflict_ignored_when_target_superseded() {
        let (_dir, lib, index, _config) = setup();
        let a = Engram::new("survivor", "b", Tier::Semantic, Status::Confirmed);
        let b = Engram::new("old fact", "b", Tier::Semantic, Status::Confirmed);
        remember(&lib, &index, &b);
        remember(&lib, &index, &a);

        let ops = crate::ops::Ops::new(&lib, &index);
        ops.link(&a.id, Rel::ConflictsConfirmed, &b.id).unwrap();
        ops.link(&a.id, Rel::Supersedes, &b.id).unwrap();

        assert!(!has_conflicts_confirmed(&index, &a.id).unwrap());
    }

    #[test]
    fn trace_walks_derived_to_told() {
        let (_dir, lib, index, _config) = setup();
        let mut told = Engram::new(
            "user said rust",
            "body",
            Tier::Episodic,
            Status::Confirmed,
        );
        told.source.push(Source {
            kind: "conversation".into(),
            r#ref: "conv_1".into(),
        });
        told.confidence = 0.95;

        let mut derived = Engram::new(
            "Alexandria is written in Rust",
            "body",
            Tier::Semantic,
            Status::Confirmed,
        );
        derived.source.push(Source {
            kind: "derived".into(),
            r#ref: told.id.clone(),
        });
        derived.confidence = 0.9;

        remember(&lib, &index, &told);
        remember(&lib, &index, &derived);

        let graph = Graph::new(&index);
        let result = graph.trace(&derived.id).unwrap();
        assert!(result.has_derived_sources);
        assert_eq!(result.effective_confidence, 0.9f64.min(0.95));
        assert!(result.nodes.iter().any(|n| n.id == told.id));
    }

    #[test]
    fn timeline_orders_and_filters() {
        let (_dir, lib, index, _config) = setup();
        let mut e1 = Engram::new("first", "b", Tier::Episodic, Status::Confirmed);
        e1.created = Utc::now() - chrono::Duration::days(2);
        e1.updated = e1.created;
        e1.last_touched = e1.created;
        let mut e2 = Engram::new("second", "b", Tier::Semantic, Status::Confirmed);
        e2.created = Utc::now() - chrono::Duration::days(1);
        e2.updated = e2.created;
        e2.last_touched = e2.created;

        remember(&lib, &index, &e1);
        remember(&lib, &index, &e2);

        let graph = Graph::new(&index);
        let all = graph.timeline(None, None, None).unwrap();
        assert_eq!(all.count, 2);
        assert_eq!(all.entries[0].claim, "first");

        let episodic = graph
            .timeline(None, None, Some(Tier::Episodic))
            .unwrap();
        assert_eq!(episodic.count, 1);
        assert_eq!(episodic.entries[0].claim, "first");
    }

    #[test]
    fn traverse_respects_depth() {
        let (_dir, lib, index, _config) = setup();
        let mut a = Engram::new("a", "b", Tier::Semantic, Status::Confirmed);
        let mut b = Engram::new("b", "b", Tier::Semantic, Status::Confirmed);
        let c = Engram::new("c", "b", Tier::Semantic, Status::Confirmed);
        a.links.push(Link {
            rel: Rel::DependsOn,
            to: b.id.clone(),
        });
        b.links.push(Link {
            rel: Rel::DependsOn,
            to: c.id.clone(),
        });
        remember(&lib, &index, &c);
        remember(&lib, &index, &b);
        remember(&lib, &index, &a);

        let graph = Graph::new(&index);
        let depth1 = graph
            .traverse(&a.id, Some(&[Rel::DependsOn]), 1)
            .unwrap();
        assert_eq!(depth1.nodes.len(), 1);
        assert_eq!(depth1.nodes[0].id, b.id);

        let depth2 = graph
            .traverse(&a.id, Some(&[Rel::DependsOn]), 2)
            .unwrap();
        assert_eq!(depth2.nodes.len(), 2);
    }
}
