use rusqlite::{params, Connection};

use crate::engram::{Engram, Rel, Status, Tier};
use crate::error::Result;
use crate::store::{Library, ParseFailure};

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS engrams(
  id TEXT PRIMARY KEY,
  tier TEXT NOT NULL,
  status TEXT NOT NULL,
  claim TEXT NOT NULL,
  body TEXT NOT NULL,
  created TEXT NOT NULL,
  updated TEXT NOT NULL,
  last_touched TEXT NOT NULL,
  confidence REAL NOT NULL,
  salience REAL NOT NULL,
  file_path TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS edges(
  from_id TEXT NOT NULL,
  rel TEXT NOT NULL,
  to_id TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS collection_members(
  engram_id TEXT NOT NULL,
  collection TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS tags(
  engram_id TEXT NOT NULL,
  tag TEXT NOT NULL
);
CREATE VIRTUAL TABLE IF NOT EXISTS engrams_fts USING fts5(
  claim,
  body,
  content='engrams',
  content_rowid='rowid'
);
CREATE INDEX IF NOT EXISTS idx_engrams_tier ON engrams(tier);
CREATE INDEX IF NOT EXISTS idx_engrams_created ON engrams(created);
CREATE INDEX IF NOT EXISTS idx_edges_from ON edges(from_id);
CREATE INDEX IF NOT EXISTS idx_edges_to ON edges(to_id);

CREATE TRIGGER IF NOT EXISTS engrams_ai AFTER INSERT ON engrams BEGIN
  INSERT INTO engrams_fts(rowid, claim, body) VALUES (new.rowid, new.claim, new.body);
END;
CREATE TRIGGER IF NOT EXISTS engrams_ad AFTER DELETE ON engrams BEGIN
  INSERT INTO engrams_fts(engrams_fts, rowid, claim, body) VALUES('delete', old.rowid, old.claim, old.body);
END;
CREATE TRIGGER IF NOT EXISTS engrams_au AFTER UPDATE ON engrams BEGIN
  INSERT INTO engrams_fts(engrams_fts, rowid, claim, body) VALUES('delete', old.rowid, old.claim, old.body);
  INSERT INTO engrams_fts(rowid, claim, body) VALUES (new.rowid, new.claim, new.body);
END;
"#;

#[derive(Debug, Clone)]
pub struct ReindexResult {
    pub indexed: usize,
    pub parse_failures: Vec<ParseFailure>,
}

pub struct Index {
    conn: Connection,
}

impl Index {
    pub fn open(library: &Library) -> Result<Self> {
        if let Some(parent) = library.index_path().parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(library.index_path())?;
        let index = Self { conn };
        index.ensure_schema()?;
        Ok(index)
    }

    fn ensure_schema(&self) -> Result<()> {
        self.conn.execute_batch(SCHEMA)?;
        Ok(())
    }

    pub fn drop_all(&self) -> Result<()> {
        self.conn.execute_batch(
            r#"
            DROP TRIGGER IF EXISTS engrams_au;
            DROP TRIGGER IF EXISTS engrams_ad;
            DROP TRIGGER IF EXISTS engrams_ai;
            DROP TABLE IF EXISTS engrams_fts;
            DROP TABLE IF EXISTS tags;
            DROP TABLE IF EXISTS collection_members;
            DROP TABLE IF EXISTS edges;
            DROP TABLE IF EXISTS engrams;
            "#,
        )?;
        self.ensure_schema()?;
        Ok(())
    }

    pub fn upsert(&self, engram: &Engram, file_path: &str) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;

        tx.execute(
            "INSERT INTO engrams (id, tier, status, claim, body, created, updated, last_touched, confidence, salience, file_path)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
             ON CONFLICT(id) DO UPDATE SET
               tier=excluded.tier, status=excluded.status, claim=excluded.claim, body=excluded.body,
               created=excluded.created, updated=excluded.updated, last_touched=excluded.last_touched,
               confidence=excluded.confidence, salience=excluded.salience, file_path=excluded.file_path",
            params![
                engram.id,
                tier_str(engram.tier),
                status_str(engram.status),
                engram.claim,
                engram.body,
                engram.created.to_rfc3339(),
                engram.updated.to_rfc3339(),
                engram.last_touched.to_rfc3339(),
                engram.confidence,
                engram.salience,
                file_path,
            ],
        )?;

        tx.execute("DELETE FROM edges WHERE from_id = ?1", params![engram.id])?;
        for link in &engram.links {
            tx.execute(
                "INSERT INTO edges (from_id, rel, to_id) VALUES (?1, ?2, ?3)",
                params![engram.id, rel_str(link.rel), link.to],
            )?;
        }

        tx.execute(
            "DELETE FROM collection_members WHERE engram_id = ?1",
            params![engram.id],
        )?;
        for collection in &engram.collections {
            tx.execute(
                "INSERT INTO collection_members (engram_id, collection) VALUES (?1, ?2)",
                params![engram.id, collection],
            )?;
        }

        tx.execute("DELETE FROM tags WHERE engram_id = ?1", params![engram.id])?;
        for tag in &engram.tags {
            tx.execute(
                "INSERT INTO tags (engram_id, tag) VALUES (?1, ?2)",
                params![engram.id, tag],
            )?;
        }

        tx.commit()?;
        Ok(())
    }

    pub fn reindex(&self, library: &Library) -> Result<ReindexResult> {
        self.drop_all()?;
        let scan = library.scan_engrams();
        for engram in &scan.engrams {
            let path = library.engram_path(engram)?;
            self.upsert(engram, &path.display().to_string())?;
        }
        Ok(ReindexResult {
            indexed: scan.engrams.len(),
            parse_failures: scan.failures,
        })
    }

    pub fn connection(&self) -> &Connection {
        &self.conn
    }
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

fn status_str(status: Status) -> &'static str {
    match status {
        Status::Confirmed => "confirmed",
        Status::Provisional => "provisional",
        Status::UnresolvedByDesign => "unresolved_by_design",
        Status::Superseded => "superseded",
        Status::Archived => "archived",
    }
}

fn rel_str(rel: Rel) -> &'static str {
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
