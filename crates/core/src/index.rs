use std::sync::{Mutex, Once};

use rusqlite::{params, Connection};
use zerocopy::IntoBytes;

use crate::config::Config;
use crate::engram::{Engram, Rel, Status, Tier};
use crate::error::{AlexandriaError, Result};
use crate::provider::{
    build_embedder_with_dim_hint, build_reranker, embed_sync, predict_embedder_id, Embedder,
    Reranker,
};
use crate::store::{Library, ParseFailure};

static VEC_EXTENSION: Once = Once::new();

fn register_vec_extension() {
    VEC_EXTENSION.call_once(|| unsafe {
        use rusqlite::ffi::sqlite3_auto_extension;
        use sqlite_vec::sqlite3_vec_init;
        #[allow(clippy::missing_transmute_annotations)]
        sqlite3_auto_extension(Some(std::mem::transmute(
            sqlite3_vec_init as *const (),
        )));
    });
}

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
CREATE TABLE IF NOT EXISTS sources(
  engram_id TEXT NOT NULL,
  kind TEXT NOT NULL,
  ref TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS collection_members(
  engram_id TEXT NOT NULL,
  collection TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS tags(
  engram_id TEXT NOT NULL,
  tag TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS index_meta(
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL
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
CREATE INDEX IF NOT EXISTS idx_sources_engram ON sources(engram_id);
CREATE INDEX IF NOT EXISTS idx_sources_ref ON sources(ref);
CREATE TABLE IF NOT EXISTS surface_triggers(
  engram_id TEXT NOT NULL,
  trigger TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_surface_triggers_trigger ON surface_triggers(trigger);
CREATE TABLE IF NOT EXISTS meta_reliability(
  domain TEXT PRIMARY KEY,
  reliability REAL NOT NULL,
  updated TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS corrections(
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  domain TEXT NOT NULL,
  engram_id TEXT,
  recorded TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS gap_outcomes(
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  domain TEXT NOT NULL,
  gap_kind TEXT NOT NULL,
  false_positive INTEGER NOT NULL,
  recorded TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS promotion_reversals(
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  engram_id TEXT NOT NULL,
  from_tier TEXT NOT NULL,
  recorded TEXT NOT NULL
);

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

const VEC_TABLE: &str = "vec_engrams";
const VEC_SHAPES_TABLE: &str = "vec_shapes";
const META_NEEDS_REEMBED: &str = "needs_reembed";
const EMBED_BATCH_SIZE: usize = 32;

#[derive(Debug, Clone)]
pub struct ReindexResult {
    pub indexed: usize,
    pub parse_failures: Vec<ParseFailure>,
}

#[derive(Debug, Clone)]
pub struct SemanticHit {
    pub id: String,
    pub claim: String,
    pub tier: String,
    pub status: String,
    pub distance: f64,
}

#[derive(Debug, Clone)]
pub struct EngramRow {
    pub id: String,
    pub tier: Tier,
    pub status: Status,
    pub claim: String,
    pub body: String,
    pub confidence: f64,
    pub collections: Vec<String>,
    pub links: Vec<(Rel, String)>,
}

pub struct Index {
    conn: Connection,
    config: Option<Config>,
    embedder: Mutex<Option<Box<dyn Embedder>>>,
    reranker: Mutex<Option<Box<dyn Reranker>>>,
}

impl Index {
    /// Open the index for read/write paths that may embed (remember, recall, reindex).
    pub fn open(library: &Library, config: &Config) -> Result<Self> {
        register_vec_extension();
        if let Some(parent) = library.index_path().parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(library.index_path())?;
        let index = Self {
            conn,
            config: Some(config.clone()),
            embedder: Mutex::new(None),
            reranker: Mutex::new(None),
        };
        index.ensure_schema()?;
        index.ensure_vec_table()?;
        index.ensure_shapes_vec_table()?;
        if index.needs_reembed() {
            index.reembed_all_engrams()?;
            index.reembed_all_shapes(library)?;
        }
        Ok(index)
    }

    /// Open for metadata-only operations (expand) without loading an embedder.
    pub fn open_readonly(library: &Library) -> Result<Self> {
        register_vec_extension();
        if let Some(parent) = library.index_path().parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(library.index_path())?;
        let index = Self {
            conn,
            config: None,
            embedder: Mutex::new(None),
            reranker: Mutex::new(None),
        };
        index.ensure_schema()?;
        Ok(index)
    }

    /// Test helper: open with a pre-built embedder (eager).
    pub fn open_with_embedder(library: &Library, embedder: Box<dyn Embedder>) -> Result<Self> {
        register_vec_extension();
        if let Some(parent) = library.index_path().parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(library.index_path())?;
        let index = Self {
            conn,
            config: None,
            embedder: Mutex::new(Some(embedder)),
            reranker: Mutex::new(None),
        };
        index.ensure_schema()?;
        index.ensure_vec_table_from_embedder()?;
        index.ensure_shapes_vec_table()?;
        if index.needs_reembed() {
            index.reembed_all_engrams()?;
            index.reembed_all_shapes(library)?;
        }
        Ok(index)
    }

    fn ensure_embedder(&self) -> Result<()> {
        let mut guard = self.embedder.lock().map_err(|e| {
            AlexandriaError::Other(anyhow::anyhow!("embedder lock poisoned: {e}"))
        })?;
        if guard.is_some() {
            return Ok(());
        }
        if self.config.is_none() {
            return Err(AlexandriaError::Config(
                "embedder not available on read-only index".into(),
            ));
        }
        let config = self.config.as_ref().unwrap();
        // Check index_meta for a cached dim matching this provider so HTTP providers
        // (OpenAI, Ollama) can skip their billed probe call on subsequent opens.
        let known_dim = self.cached_dim_for_config(config);
        *guard = Some(build_embedder_with_dim_hint(config, known_dim)?);
        Ok(())
    }

    /// Return the dim stored in `index_meta` if the stored embedder id matches what
    /// `config` would produce, so HTTP providers can skip their probe call.
    fn cached_dim_for_config(&self, config: &Config) -> Option<usize> {
        let expected_id = predict_embedder_id(config)?;
        let stored_id: String = self
            .conn
            .query_row(
                "SELECT value FROM index_meta WHERE key = 'embedder_id'",
                [],
                |row| row.get(0),
            )
            .ok()?;
        if stored_id != expected_id {
            return None;
        }
        self.stored_embedding_dim().ok().flatten()
    }

    fn with_embedder<R>(&self, f: impl FnOnce(&dyn Embedder) -> Result<R>) -> Result<R> {
        self.ensure_embedder()?;
        let guard = self.embedder.lock().map_err(|e| {
            AlexandriaError::Other(anyhow::anyhow!("embedder lock poisoned: {e}"))
        })?;
        let embedder = guard.as_ref().unwrap();
        f(embedder.as_ref())
    }

    fn ensure_reranker(&self) -> Result<()> {
        let mut guard = self.reranker.lock().map_err(|e| {
            AlexandriaError::Other(anyhow::anyhow!("reranker lock poisoned: {e}"))
        })?;
        if guard.is_some() {
            return Ok(());
        }
        let Some(config) = self.config.as_ref() else {
            return Ok(());
        };
        if let Some(reranker) = build_reranker(config)? {
            *guard = Some(reranker);
        }
        Ok(())
    }

    pub fn with_reranker<R>(&self, f: impl FnOnce(Option<&dyn Reranker>) -> Result<R>) -> Result<R> {
        self.ensure_reranker()?;
        let guard = self.reranker.lock().map_err(|e| {
            AlexandriaError::Other(anyhow::anyhow!("reranker lock poisoned: {e}"))
        })?;
        match guard.as_ref() {
            Some(r) => f(Some(r.as_ref())),
            None => f(None),
        }
    }

    fn ensure_schema(&self) -> Result<()> {
        self.conn.execute_batch(SCHEMA)?;
        Ok(())
    }

    fn stored_embedding_dim(&self) -> Result<Option<usize>> {
        let dim: Option<String> = self
            .conn
            .query_row(
                "SELECT value FROM index_meta WHERE key = 'embedder_dim'",
                [],
                |row| row.get(0),
            )
            .ok();
        Ok(dim.and_then(|s| s.parse().ok()))
    }

    fn ensure_vec_table(&self) -> Result<()> {
        self.with_embedder(|embedder| self.ensure_vec_table_inner(embedder))
    }

    fn ensure_vec_table_from_embedder(&self) -> Result<()> {
        let guard = self.embedder.lock().map_err(|e| {
            AlexandriaError::Other(anyhow::anyhow!("embedder lock poisoned: {e}"))
        })?;
        let embedder = guard.as_deref().ok_or_else(|| {
            AlexandriaError::Config("embedder required for vec table setup".into())
        })?;
        self.ensure_vec_table_inner(embedder)
    }

    fn ensure_vec_table_inner(&self, embedder: &dyn Embedder) -> Result<()> {
        let dim = embedder.dim();
        let stored_id: Option<String> = self
            .conn
            .query_row(
                "SELECT value FROM index_meta WHERE key = 'embedder_id'",
                [],
                |row| row.get(0),
            )
            .ok();
        let current_id = embedder.id().to_string();
        let stored_dim = self.stored_embedding_dim()?;

        let mismatch = stored_id.as_deref() != Some(current_id.as_str())
            || stored_dim != Some(dim);

        if mismatch {
            self.drop_vec_table()?;
            self.conn.execute_batch(&format!(
                "CREATE VIRTUAL TABLE IF NOT EXISTS {VEC_TABLE} USING vec0(embedding float[{dim}])"
            ))?;
            self.set_meta("embedder_id", &current_id)?;
            self.set_meta("embedder_dim", &dim.to_string())?;
            self.set_meta(META_NEEDS_REEMBED, "1")?;
        } else if !self.vec_table_exists()? {
            self.conn.execute_batch(&format!(
                "CREATE VIRTUAL TABLE IF NOT EXISTS {VEC_TABLE} USING vec0(embedding float[{dim}])"
            ))?;
            self.set_meta("embedder_id", &current_id)?;
            self.set_meta("embedder_dim", &dim.to_string())?;
        }
        Ok(())
    }

    fn set_meta(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO index_meta (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn needs_reembed(&self) -> bool {
        self.conn
            .query_row(
                "SELECT value FROM index_meta WHERE key = ?1",
                params![META_NEEDS_REEMBED],
                |row| row.get::<_, String>(0),
            )
            .map(|v| v == "1")
            .unwrap_or(false)
    }

    fn vec_table_exists(&self) -> Result<bool> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
            params![VEC_TABLE],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    fn drop_vec_table(&self) -> Result<()> {
        let _ = self
            .conn
            .execute_batch(&format!("DROP TABLE IF EXISTS {VEC_TABLE}"));
        Ok(())
    }

    pub fn drop_all(&self) -> Result<()> {
        self.conn.execute_batch(
            r#"
            DROP TRIGGER IF EXISTS engrams_au;
            DROP TRIGGER IF EXISTS engrams_ad;
            DROP TRIGGER IF EXISTS engrams_ai;
            DROP TABLE IF EXISTS engrams_fts;
            DROP TABLE IF EXISTS surface_triggers;
            DROP TABLE IF EXISTS promotion_reversals;
            DROP TABLE IF EXISTS gap_outcomes;
            DROP TABLE IF EXISTS corrections;
            DROP TABLE IF EXISTS meta_reliability;
            DROP TABLE IF EXISTS tags;
            DROP TABLE IF EXISTS collection_members;
            DROP TABLE IF EXISTS sources;
            DROP TABLE IF EXISTS edges;
            DROP TABLE IF EXISTS engrams;
            DROP TABLE IF EXISTS index_meta;
            "#,
        )?;
        self.drop_vec_table()?;
        self.drop_shapes_vec_table()?;
        self.ensure_schema()?;
        if self.config.is_some() {
            self.ensure_vec_table()?;
            self.ensure_shapes_vec_table()?;
        } else if let Ok(guard) = self.embedder.lock() {
            if let Some(embedder) = guard.as_deref() {
                self.ensure_vec_table_inner(embedder)?;
                self.ensure_shapes_vec_table_inner(embedder)?;
            }
        }
        Ok(())
    }

    pub fn upsert(&self, engram: &Engram, file_path: &str) -> Result<()> {
        self.upsert_inner(engram, file_path, true)
    }

    fn upsert_inner(&self, engram: &Engram, file_path: &str, embed: bool) -> Result<()> {
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

        let rowid: i64 = tx.query_row(
            "SELECT rowid FROM engrams WHERE id = ?1",
            params![engram.id],
            |row| row.get(0),
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

        tx.execute(
            "DELETE FROM sources WHERE engram_id = ?1",
            params![engram.id],
        )?;
        for source in &engram.source {
            tx.execute(
                "INSERT INTO sources (engram_id, kind, ref) VALUES (?1, ?2, ?3)",
                params![engram.id, source.kind, source.r#ref],
            )?;
        }

        tx.execute(
            "DELETE FROM surface_triggers WHERE engram_id = ?1",
            params![engram.id],
        )?;
        if let Some(triggers) = &engram.surface_when {
            for trigger in triggers {
                tx.execute(
                    "INSERT INTO surface_triggers (engram_id, trigger) VALUES (?1, ?2)",
                    params![engram.id, trigger],
                )?;
            }
        }

        tx.commit()?;

        if embed {
            if engram.tier != Tier::Relational {
                self.upsert_embedding(rowid, engram)?;
            } else {
                let _ = self.conn.execute(
                    &format!("DELETE FROM {VEC_TABLE} WHERE rowid = ?1"),
                    params![rowid],
                );
            }
        }

        Ok(())
    }

    pub fn get_sources(&self, engram_id: &str) -> Result<Vec<crate::engram::Source>> {
        let mut stmt = self.conn.prepare(
            "SELECT kind, ref FROM sources WHERE engram_id = ?1 ORDER BY rowid",
        )?;
        let rows = stmt.query_map(params![engram_id], |row| {
            Ok(crate::engram::Source {
                kind: row.get(0)?,
                r#ref: row.get(1)?,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    fn embed_text(engram: &Engram) -> String {
        if engram.body.trim().is_empty() {
            engram.claim.clone()
        } else {
            format!("{}\n{}", engram.claim, engram.body)
        }
    }

    pub fn reembed_all_engrams(&self) -> Result<()> {
        let mut stmt = self.conn.prepare("SELECT rowid, claim, body, tier FROM engrams")?;
        let rows: Vec<(i64, String, String, String)> = stmt
            .query_map([], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        let mut pending: Vec<(i64, String)> = Vec::new();
        for (rowid, claim, body, tier_s) in rows {
            if tier_s == "relational" {
                continue;
            }
            let text = if body.trim().is_empty() {
                claim
            } else {
                format!("{claim}\n{body}")
            };
            pending.push((rowid, text));
        }

        self.with_embedder(|embedder| {
            for chunk in pending.chunks(EMBED_BATCH_SIZE) {
                let texts: Vec<String> = chunk.iter().map(|(_, t)| t.clone()).collect();
                let vectors = embed_sync(embedder, &texts)?;
                for ((rowid, _), embedding) in chunk.iter().zip(vectors.iter()) {
                    let _ = self.conn.execute(
                        &format!("DELETE FROM {VEC_TABLE} WHERE rowid = ?1"),
                        params![rowid],
                    );
                    self.conn.execute(
                        &format!("INSERT INTO {VEC_TABLE}(rowid, embedding) VALUES (?1, ?2)"),
                        params![rowid, embedding.as_bytes()],
                    )?;
                }
            }
            Ok(())
        })?;

        self.set_meta(META_NEEDS_REEMBED, "0")?;
        Ok(())
    }

    fn upsert_embedding(&self, rowid: i64, engram: &Engram) -> Result<()> {
        let text = Self::embed_text(engram);
        self.with_embedder(|embedder| {
            let vectors = embed_sync(embedder, &[text])?;
            let embedding = &vectors[0];
            let _ = self.conn.execute(
                &format!("DELETE FROM {VEC_TABLE} WHERE rowid = ?1"),
                params![rowid],
            );
            self.conn.execute(
                &format!("INSERT INTO {VEC_TABLE}(rowid, embedding) VALUES (?1, ?2)"),
                params![rowid, embedding.as_bytes()],
            )?;
            Ok(())
        })
    }

    pub fn reindex(&self, library: &Library) -> Result<ReindexResult> {
        self.drop_all()?;
        let scan = library.scan_engrams();
        for engram in &scan.engrams {
            let path = library.engram_path(engram)?;
            self.upsert_inner(engram, &path.display().to_string(), false)?;
        }
        if self.config.is_some() || self.needs_reembed() {
            self.reembed_all_engrams()?;
            self.reembed_all_shapes(library)?;
        }
        Ok(ReindexResult {
            indexed: scan.engrams.len(),
            parse_failures: scan.failures,
        })
    }

    pub fn semantic_knn(&self, query_vec: &[f32], limit: u32) -> Result<Vec<SemanticHit>> {
        if !self.vec_table_exists()? {
            return Ok(Vec::new());
        }
        let mut stmt = self.conn.prepare(&format!(
            r#"
            SELECT e.id, e.claim, e.tier, e.status, v.distance
            FROM {VEC_TABLE} v
            JOIN engrams e ON e.rowid = v.rowid
            WHERE v.embedding MATCH ?1
              AND k = ?2
              AND e.tier != 'relational'
            ORDER BY distance
            "#
        ))?;
        let rows = stmt.query_map(params![query_vec.as_bytes(), limit], |row| {
            Ok(SemanticHit {
                id: row.get(0)?,
                claim: row.get(1)?,
                tier: row.get(2)?,
                status: row.get(3)?,
                distance: row.get(4)?,
            })
        })?;
        let mut hits = Vec::new();
        for row in rows {
            hits.push(row?);
        }
        Ok(hits)
    }

    pub fn neighbors_within(&self, query_vec: &[f32], radius: f32) -> Result<u32> {
        let limit = 100u32;
        let hits = self.semantic_knn(query_vec, limit)?;
        let count = hits
            .iter()
            .filter(|h| (h.distance as f32) < radius)
            .count();
        Ok(count as u32)
    }

    pub fn nearest_collection_centroid(
        &self,
        query_vec: &[f32],
    ) -> Result<Option<(String, f32)>> {
        let mut stmt = self.conn.prepare(
            "SELECT DISTINCT collection FROM collection_members ORDER BY collection",
        )?;
        let collections: Vec<String> = stmt
            .query_map([], |row| row.get(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        let mut best: Option<(String, f32)> = None;
        for collection in collections {
            let centroid = self.collection_centroid(&collection)?;
            let Some(centroid) = centroid else { continue };
            let dist = l2_distance(query_vec, &centroid);
            if best.as_ref().map(|(_, d)| dist < *d).unwrap_or(true) {
                best = Some((collection, dist));
            }
        }
        Ok(best)
    }

    fn collection_centroid(&self, collection: &str) -> Result<Option<Vec<f32>>> {
        if !self.vec_table_exists()? {
            return Ok(None);
        }
        let mut stmt = self.conn.prepare(&format!(
            r#"
            SELECT v.embedding
            FROM {VEC_TABLE} v
            JOIN engrams e ON e.rowid = v.rowid
            JOIN collection_members cm ON cm.engram_id = e.id
            WHERE cm.collection = ?1
              AND e.tier != 'relational'
            "#
        ))?;
        let rows = stmt.query_map(params![collection], |row| {
            let blob: Vec<u8> = row.get(0)?;
            Ok(blob_to_f32(&blob))
        })?;
        let mut vectors = Vec::new();
        for row in rows {
            if let Some(v) = row? {
                vectors.push(v);
            }
        }
        if vectors.is_empty() {
            return Ok(None);
        }
        let dim = vectors[0].len();
        let mut sum = vec![0.0f32; dim];
        for v in &vectors {
            for (i, x) in v.iter().enumerate() {
                sum[i] += x;
            }
        }
        let n = vectors.len() as f32;
        for x in &mut sum {
            *x /= n;
        }
        Ok(Some(sum))
    }

    pub fn file_path(&self, id: &str) -> Result<Option<String>> {
        let row = self.conn.query_row(
            "SELECT file_path FROM engrams WHERE id = ?1",
            params![id],
            |row| row.get(0),
        );
        match row {
            Ok(path) => Ok(Some(path)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub fn get_engram(&self, id: &str) -> Result<Option<EngramRow>> {
        let row = self.conn.query_row(
            "SELECT id, tier, status, claim, body, confidence FROM engrams WHERE id = ?1",
            params![id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, f64>(5)?,
                ))
            },
        );

        let (id, tier_s, status_s, claim, body, confidence) = match row {
            Ok(r) => r,
            Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(None),
            Err(e) => return Err(e.into()),
        };

        let tier = Tier::parse(&tier_s)?;
        let status = Status::parse(&status_s)?;

        let mut stmt = self
            .conn
            .prepare("SELECT collection FROM collection_members WHERE engram_id = ?1")?;
        let collections: Vec<String> = stmt
            .query_map(params![&id], |row| row.get(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        let mut links = Vec::new();
        let mut link_stmt = self
            .conn
            .prepare("SELECT rel, to_id FROM edges WHERE from_id = ?1")?;
        let link_rows = link_stmt.query_map(params![&id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        for row in link_rows {
            let (rel_s, to) = row?;
            links.push((parse_rel(&rel_s)?, to));
        }

        Ok(Some(EngramRow {
            id,
            tier,
            status,
            claim,
            body,
            confidence,
            collections,
            links,
        }))
    }

    pub fn get_linked_claims(
        &self,
        from_id: &str,
        rel_filter: Option<Rel>,
    ) -> Result<Vec<(Rel, String, String)>> {
        let (sql, rel_param) = if let Some(rel) = rel_filter {
            (
                "SELECT e.rel, e.to_id, t.claim FROM edges e JOIN engrams t ON t.id = e.to_id WHERE e.from_id = ?1 AND e.rel = ?2",
                Some(rel_str(rel).to_string()),
            )
        } else {
            (
                "SELECT e.rel, e.to_id, t.claim FROM edges e JOIN engrams t ON t.id = e.to_id WHERE e.from_id = ?1",
                None,
            )
        };
        let mut stmt = self.conn.prepare(sql)?;
        let mut out = Vec::new();
        if let Some(rel_s) = rel_param {
            let rows = stmt.query_map(params![from_id, rel_s], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })?;
            for row in rows {
                let (rel_s, to_id, claim) = row?;
                out.push((parse_rel(&rel_s)?, to_id, claim));
            }
        } else {
            let rows = stmt.query_map(params![from_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })?;
            for row in rows {
                let (rel_s, to_id, claim) = row?;
                out.push((parse_rel(&rel_s)?, to_id, claim));
            }
        }
        Ok(out)
    }

    pub fn embedder_id(&self) -> Result<String> {
        if let Ok(guard) = self.embedder.lock() {
            if let Some(e) = guard.as_ref() {
                return Ok(e.id().to_string());
            }
        }
        self.conn
            .query_row(
                "SELECT value FROM index_meta WHERE key = 'embedder_id'",
                [],
                |row| row.get(0),
            )
            .map_err(Into::into)
    }

    pub fn embed_query(&self, query: &str) -> Result<Vec<f32>> {
        self.with_embedder(|embedder| {
            let vectors = embed_sync(embedder, &[query.to_string()])?;
            Ok(vectors.into_iter().next().unwrap_or_default())
        })
    }

    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    pub fn has_conflict_edges_among(&self, ids: &[String]) -> Result<bool> {
        for id in ids {
            let count: i64 = self.conn.query_row(
                "SELECT COUNT(*) FROM edges WHERE from_id = ?1 AND rel IN ('conflicts_confirmed', 'tension_possible')",
                params![id],
                |row| row.get(0),
            )?;
            if count > 0 {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub fn engrams_matching_surface_trigger(&self, topic: &str) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare("SELECT DISTINCT engram_id, trigger FROM surface_triggers")?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut ids = Vec::new();
        for row in rows {
            let (id, trigger) = row?;
            if surface_trigger_matches(&trigger, topic) && !ids.contains(&id) {
                ids.push(id);
            }
        }
        Ok(ids)
    }

    fn drop_shapes_vec_table(&self) -> Result<()> {
        let _ = self
            .conn
            .execute_batch(&format!("DROP TABLE IF EXISTS {VEC_SHAPES_TABLE}"));
        Ok(())
    }

    fn shapes_vec_table_exists(&self) -> Result<bool> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
            params![VEC_SHAPES_TABLE],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    pub fn ensure_shapes_vec_table(&self) -> Result<()> {
        self.with_embedder(|embedder| self.ensure_shapes_vec_table_inner(embedder))
    }

    fn ensure_shapes_vec_table_inner(&self, embedder: &dyn Embedder) -> Result<()> {
        let dim = embedder.dim();
        if !self.shapes_vec_table_exists()? {
            self.conn.execute_batch(&format!(
                "CREATE VIRTUAL TABLE IF NOT EXISTS {VEC_SHAPES_TABLE} USING vec0(embedding float[{dim}])"
            ))?;
        }
        Ok(())
    }

    pub fn upsert_shape_embedding(&self, engram_id: &str, shape_text: &str) -> Result<()> {
        let rowid: i64 = self
            .conn
            .query_row(
                "SELECT rowid FROM engrams WHERE id = ?1",
                params![engram_id],
                |row| row.get(0),
            )
            .map_err(|_| AlexandriaError::EngramNotFound(engram_id.to_string()))?;
        self.with_embedder(|embedder| {
            let vectors = embed_sync(embedder, &[shape_text.to_string()])?;
            let embedding = &vectors[0];
            let _ = self.conn.execute(
                &format!("DELETE FROM {VEC_SHAPES_TABLE} WHERE rowid = ?1"),
                params![rowid],
            );
            self.conn.execute(
                &format!("INSERT INTO {VEC_SHAPES_TABLE}(rowid, embedding) VALUES (?1, ?2)"),
                params![rowid, embedding.as_bytes()],
            )?;
            Ok(())
        })
    }

    pub fn reembed_all_shapes(&self, library: &Library) -> Result<()> {
        self.ensure_shapes_vec_table()?;
        let scan = library.scan_engrams();
        for engram in &scan.engrams {
            if engram.tier != Tier::Episodic {
                continue;
            }
            if let Some(ref shape_ref) = engram.shape_ref {
                if let Ok(content) = std::fs::read_to_string(library.engram_path(engram)?) {
                    if let Ok(parsed) = Engram::parse(&content) {
                        let summary = if parsed.body.contains("Shape:") {
                            parsed.body.clone()
                        } else {
                            format!("Shape: {shape_ref}")
                        };
                        let _ = self.upsert_shape_embedding(&engram.id, &summary);
                    }
                }
            } else {
                let summary = crate::shape::extract_shape_summary_heuristic(engram);
                let _ = self.upsert_shape_embedding(&engram.id, &summary);
            }
        }
        Ok(())
    }

    pub fn shape_knn(&self, query_vec: &[f32], limit: u32) -> Result<Vec<SemanticHit>> {
        if !self.shapes_vec_table_exists()? {
            return Ok(Vec::new());
        }
        let mut stmt = self.conn.prepare(&format!(
            r#"
            SELECT e.id, e.claim, e.tier, e.status, v.distance
            FROM {VEC_SHAPES_TABLE} v
            JOIN engrams e ON e.rowid = v.rowid
            WHERE v.embedding MATCH ?1
              AND k = ?2
              AND e.tier = 'episodic'
            ORDER BY distance
            "#
        ))?;
        let rows = stmt.query_map(params![query_vec.as_bytes(), limit], |row| {
            Ok(SemanticHit {
                id: row.get(0)?,
                claim: row.get(1)?,
                tier: row.get(2)?,
                status: row.get(3)?,
                distance: row.get(4)?,
            })
        })?;
        let mut hits = Vec::new();
        for row in rows {
            hits.push(row?);
        }
        Ok(hits)
    }

    pub fn clear_meta_tables(&self) -> Result<()> {
        self.conn.execute_batch(
            "DELETE FROM meta_reliability; DELETE FROM corrections; DELETE FROM gap_outcomes; DELETE FROM promotion_reversals;",
        )?;
        Ok(())
    }

    pub fn insert_correction(
        &self,
        domain: &str,
        engram_id: Option<&str>,
        timestamp: &chrono::DateTime<chrono::Utc>,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO corrections (domain, engram_id, recorded) VALUES (?1, ?2, ?3)",
            params![domain, engram_id, timestamp.to_rfc3339()],
        )?;
        Ok(())
    }

    pub fn insert_gap_outcome(
        &self,
        domain: &str,
        gap_kind: &str,
        false_positive: bool,
        timestamp: &chrono::DateTime<chrono::Utc>,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO gap_outcomes (domain, gap_kind, false_positive, recorded) VALUES (?1, ?2, ?3, ?4)",
            params![
                domain,
                gap_kind,
                i32::from(false_positive),
                timestamp.to_rfc3339()
            ],
        )?;
        Ok(())
    }

    pub fn insert_promotion_reversal(
        &self,
        engram_id: &str,
        from_tier: &str,
        timestamp: &chrono::DateTime<chrono::Utc>,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO promotion_reversals (engram_id, from_tier, recorded) VALUES (?1, ?2, ?3)",
            params![engram_id, from_tier, timestamp.to_rfc3339()],
        )?;
        Ok(())
    }

    /// Rebuild per-domain reliability from the meta event tables.
    ///
    /// Formula (v1 heuristic, tunable):
    ///   gap_penalty = (false_positive gaps) / (total gap outcomes)  [0 if no gaps]
    ///   reliability = clamp(1.0 - min(corr_count * 0.1, 0.5) - gap_penalty * 0.3, 0..1)
    ///
    /// M5 uses reliability for bounded calibration (score down-weighting in recall) and posture.
    /// Full live per-domain threshold self-tuning is intentionally deferred.
    ///
    /// Default `posture.meta_reliability_threshold` is 0.6 because corrections alone floor
    /// reliability at 0.5 (penalty capped at 0.5); threshold must exceed that to fire.
    pub fn recompute_meta_reliability(&self) -> Result<()> {
        self.conn.execute("DELETE FROM meta_reliability", [])?;
        let mut stmt = self.conn.prepare(
            "SELECT domain,
                    (SELECT COUNT(*) FROM corrections c WHERE c.domain = d.domain) as corr,
                    (SELECT COUNT(*) FROM gap_outcomes g WHERE g.domain = d.domain AND g.false_positive = 1) as gap_fp,
                    (SELECT COUNT(*) FROM gap_outcomes g2 WHERE g2.domain = d.domain) as gap_total,
                    (SELECT COUNT(*) FROM promotion_reversals p) as rev
             FROM (SELECT DISTINCT domain FROM corrections
                   UNION SELECT DISTINCT domain FROM gap_outcomes) d",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
            ))
        })?;
        let now = chrono::Utc::now().to_rfc3339();
        for row in rows {
            let (domain, corr, gap_fp, gap_total, _rev) = row?;
            let gap_penalty = if gap_total > 0 {
                gap_fp as f64 / gap_total as f64
            } else {
                0.0
            };
            let reliability = (1.0 - (corr as f64 * 0.1).min(0.5) - gap_penalty * 0.3).clamp(0.0, 1.0);
            self.conn.execute(
                "INSERT INTO meta_reliability (domain, reliability, updated) VALUES (?1, ?2, ?3)",
                params![domain, reliability, now],
            )?;
        }
        Ok(())
    }

    pub fn meta_reliability(&self, domain: Option<&str>) -> Result<f64> {
        match domain {
            Some(d) => {
                let row = self.conn.query_row(
                    "SELECT reliability FROM meta_reliability WHERE domain = ?1",
                    params![d],
                    |row| row.get::<_, f64>(0),
                );
                match row {
                    Ok(r) => Ok(r),
                    Err(rusqlite::Error::QueryReturnedNoRows) => Ok(1.0),
                    Err(e) => Err(e.into()),
                }
            }
            None => {
                let count: i64 = self.conn.query_row(
                    "SELECT COUNT(*) FROM meta_reliability",
                    [],
                    |row| row.get(0),
                )?;
                if count == 0 {
                    return Ok(1.0);
                }
                self.conn
                    .query_row(
                        "SELECT AVG(reliability) FROM meta_reliability",
                        [],
                        |row| row.get::<_, f64>(0),
                    )
                    .map_err(Into::into)
            }
        }
    }

    pub fn recent_corrections_count(&self, domain: Option<&str>, days: i64) -> Result<u32> {
        let cutoff = (chrono::Utc::now() - chrono::Duration::days(days)).to_rfc3339();
        let count: i64 = match domain {
            Some(d) => self.conn.query_row(
                "SELECT COUNT(*) FROM corrections WHERE domain = ?1 AND recorded >= ?2",
                params![d, cutoff],
                |row| row.get(0),
            )?,
            None => self.conn.query_row(
                "SELECT COUNT(*) FROM corrections WHERE recorded >= ?1",
                params![cutoff],
                |row| row.get(0),
            )?,
        };
        Ok(count as u32)
    }

    pub fn gap_false_positive_rate(&self, domain: Option<&str>) -> Result<(f64, u32)> {
        let (fp, total): (i64, i64) = match domain {
            Some(d) => self.conn.query_row(
                "SELECT COALESCE(SUM(false_positive),0), COUNT(*) FROM gap_outcomes WHERE domain = ?1",
                params![d],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )?,
            None => self.conn.query_row(
                "SELECT COALESCE(SUM(false_positive),0), COUNT(*) FROM gap_outcomes",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )?,
        };
        let rate = if total > 0 {
            fp as f64 / total as f64
        } else {
            0.0
        };
        Ok((rate, total as u32))
    }

    pub fn promotion_reversal_rate(&self, domain: Option<&str>) -> Result<(f64, u32)> {
        let total: i64 = match domain {
            Some(_) => self.conn.query_row(
                "SELECT COUNT(*) FROM promotion_reversals",
                [],
                |row| row.get(0),
            )?,
            None => self.conn.query_row(
                "SELECT COUNT(*) FROM promotion_reversals",
                [],
                |row| row.get(0),
            )?,
        };
        let rate = if total > 0 {
            (total as f64 * 0.1).min(1.0)
        } else {
            0.0
        };
        Ok((rate, total as u32))
    }

    pub fn total_corrections(&self, domain: Option<&str>) -> Result<u32> {
        let count: i64 = match domain {
            Some(d) => self.conn.query_row(
                "SELECT COUNT(*) FROM corrections WHERE domain = ?1",
                params![d],
                |row| row.get(0),
            )?,
            None => self.conn.query_row(
                "SELECT COUNT(*) FROM corrections",
                [],
                |row| row.get(0),
            )?,
        };
        Ok(count as u32)
    }
}

fn surface_trigger_matches(trigger: &str, topic: &str) -> bool {
    let topic_lower = topic.to_lowercase();
    let trigger_lower = trigger.to_lowercase();
    if let Some(rest) = trigger_lower.strip_prefix("topic:") {
        topic_lower.contains(rest) || rest.contains(&topic_lower)
    } else {
        trigger_lower.contains(&topic_lower) || topic_lower.contains(&trigger_lower)
    }
}

fn blob_to_f32(blob: &[u8]) -> Option<Vec<f32>> {
    if !blob.len().is_multiple_of(4) {
        return None;
    }
    let mut out = Vec::with_capacity(blob.len() / 4);
    for chunk in blob.chunks_exact(4) {
        out.push(f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
    }
    Some(out)
}

fn l2_distance(a: &[f32], b: &[f32]) -> f32 {
    a.iter()
        .zip(b.iter())
        .map(|(x, y)| {
            let d = x - y;
            d * d
        })
        .sum::<f32>()
        .sqrt()
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

fn parse_rel(s: &str) -> Result<Rel> {
    match s {
        "supports" => Ok(Rel::Supports),
        "refines" => Ok(Rel::Refines),
        "depends_on" => Ok(Rel::DependsOn),
        "caused_by" => Ok(Rel::CausedBy),
        "conflicts_confirmed" => Ok(Rel::ConflictsConfirmed),
        "tension_possible" => Ok(Rel::TensionPossible),
        "context_qualified" => Ok(Rel::ContextQualified),
        "coexists" => Ok(Rel::Coexists),
        "supersedes" => Ok(Rel::Supersedes),
        "superseded_by" => Ok(Rel::SupersededBy),
        "aspect_of" => Ok(Rel::AspectOf),
        "same_episode" => Ok(Rel::SameEpisode),
        _ => Err(AlexandriaError::InvalidEngram(format!(
            "unknown rel: {s}"
        ))),
    }
}
