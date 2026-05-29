use std::fs;
use std::path::{Path, PathBuf};

use walkdir::WalkDir;

use crate::config::Config;
use crate::engram::{Engram, Status, Tier};
use crate::error::{AlexandriaError, Result};

const ALEXANDRIA_DIR: &str = ".alexandria";

const TIER_DIRS: &[&str] = &[
    "episodic",
    "provisional",
    "semantic",
    "procedural",
    "relational",
    "threads",
    "collections",
    "archive",
];

#[derive(Debug, Clone)]
pub struct Library {
    pub root: PathBuf,
}

#[derive(Debug, Clone)]
pub struct ParseFailure {
    pub path: PathBuf,
    pub error: String,
}

#[derive(Debug, Clone)]
pub struct ScanResult {
    pub engrams: Vec<Engram>,
    pub failures: Vec<ParseFailure>,
}

impl Library {
    pub fn discover(start: Option<&Path>) -> Result<Self> {
        let start = start
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

        let start_display = start.display().to_string();
        let mut current = start.canonicalize().unwrap_or(start);
        loop {
            if current.join(ALEXANDRIA_DIR).is_dir() {
                return Ok(Self { root: current });
            }
            if !current.pop() {
                break;
            }
        }

        Err(AlexandriaError::LibraryNotFound(start_display))
    }

    pub fn init(path: &Path) -> Result<Self> {
        let root = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        if root.join(ALEXANDRIA_DIR).exists() {
            return Err(AlexandriaError::LibraryAlreadyExists(
                root.display().to_string(),
            ));
        }

        fs::create_dir_all(root.join(ALEXANDRIA_DIR))?;
        fs::create_dir_all(root.join(ALEXANDRIA_DIR).join("fast_reflections"))?;
        fs::create_dir_all(root.join(ALEXANDRIA_DIR).join("meta_log"))?;
        for dir in TIER_DIRS {
            fs::create_dir_all(root.join(dir))?;
        }
        Config::write_default(&root)?;

        Ok(Self { root })
    }

    pub fn alexandria_dir(&self) -> PathBuf {
        self.root.join(ALEXANDRIA_DIR)
    }

    pub fn index_path(&self) -> PathBuf {
        self.alexandria_dir().join("index.db")
    }

    pub fn tier_dir(&self, tier: Tier) -> Result<PathBuf> {
        match tier {
            Tier::Working => Err(AlexandriaError::EphemeralTier("working".into())),
            Tier::Episodic => Ok(self.root.join("episodic")),
            Tier::Provisional => Ok(self.root.join("provisional")),
            Tier::Semantic => Ok(self.root.join("semantic")),
            Tier::Procedural => Ok(self.root.join("procedural")),
            Tier::Relational => Ok(self.root.join("relational")),
        }
    }

    pub fn engram_path(&self, engram: &Engram) -> Result<PathBuf> {
        let dir = if engram.status == Status::UnresolvedByDesign {
            self.root.join("threads")
        } else if engram.status == Status::Archived || engram.status == Status::Superseded {
            self.root.join("archive")
        } else {
            self.tier_dir(engram.tier)?
        };
        Ok(dir.join(format!("{}.md", engram.id)))
    }

    /// Write an engram, relocating the file when tier/status changes the target path.
    /// Deletes the old file when the path changes.
    pub fn save_relocating(&self, engram: &Engram, old_path: Option<&Path>) -> Result<PathBuf> {
        if engram.tier == Tier::Working {
            return Err(AlexandriaError::EphemeralTier("working".into()));
        }
        let new_path = self.engram_path(engram)?;
        if let Some(old) = old_path {
            let old_canon = old.canonicalize().unwrap_or_else(|_| old.to_path_buf());
            let new_canon = new_path.canonicalize().unwrap_or_else(|_| new_path.clone());
            if old_canon != new_canon && old.exists() {
                fs::remove_file(old)?;
            }
        }
        if new_path.exists() {
            let existing = self.read_engram(&new_path)?;
            if existing.claim != engram.claim || existing.created != engram.created {
                return Err(AlexandriaError::IdCollision {
                    id: engram.id.clone(),
                    path: new_path.display().to_string(),
                    existing_claim: existing.claim,
                });
            }
        }
        if let Some(parent) = new_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let content = engram.serialize()?;
        fs::write(&new_path, content)?;
        Ok(new_path)
    }

    pub fn write_engram(&self, engram: &Engram) -> Result<PathBuf> {
        if engram.tier == Tier::Working {
            return Err(AlexandriaError::EphemeralTier("working".into()));
        }
        let path = self.engram_path(engram)?;
        if path.exists() {
            let existing = self.read_engram(&path)?;
            if existing.claim != engram.claim || existing.created != engram.created {
                return Err(AlexandriaError::IdCollision {
                    id: engram.id.clone(),
                    path: path.display().to_string(),
                    existing_claim: existing.claim,
                });
            }
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let content = engram.serialize()?;
        fs::write(&path, content)?;
        Ok(path)
    }

    pub fn read_engram(&self, path: &Path) -> Result<Engram> {
        let content = fs::read_to_string(path)?;
        Engram::parse(&content)
    }

    /// Scan all markdown engrams, collecting parse failures instead of dropping them.
    pub fn scan_engrams(&self) -> ScanResult {
        let mut engrams = Vec::new();
        let mut failures = Vec::new();

        for entry in WalkDir::new(&self.root)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
        {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("md") {
                continue;
            }
            if path
                .components()
                .any(|c| c.as_os_str() == ALEXANDRIA_DIR)
            {
                continue;
            }
            match self.read_engram(path) {
                Ok(engram) => engrams.push(engram),
                Err(e) => failures.push(ParseFailure {
                    path: path.to_path_buf(),
                    error: e.to_string(),
                }),
            }
        }

        ScanResult { engrams, failures }
    }

    pub fn iter_engrams(&self) -> Result<Vec<Engram>> {
        Ok(self.scan_engrams().engrams)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engram::Status;

    #[test]
    fn init_creates_layout() {
        let dir = tempfile::TempDir::new().unwrap();
        let lib = Library::init(dir.path()).unwrap();
        assert!(lib.alexandria_dir().is_dir());
        assert!(lib.root.join("semantic").is_dir());
        assert!(lib.root.join("threads").is_dir());
        assert!(Config::load(dir.path()).unwrap().providers.embedder == "fastembed");
    }

    #[test]
    fn tier_dir_mapping() {
        let dir = tempfile::TempDir::new().unwrap();
        let lib = Library::init(dir.path()).unwrap();
        assert!(lib.tier_dir(Tier::Working).is_err());
        assert_eq!(
            lib.tier_dir(Tier::Semantic)
                .unwrap()
                .canonicalize()
                .unwrap(),
            dir.path().join("semantic").canonicalize().unwrap()
        );
    }

    #[test]
    fn write_and_read_round_trip() {
        let dir = tempfile::TempDir::new().unwrap();
        let lib = Library::init(dir.path()).unwrap();
        let engram = Engram::new("test claim", "body text", Tier::Semantic, Status::Confirmed);
        let path = lib.write_engram(&engram).unwrap();
        let read = lib.read_engram(&path).unwrap();
        assert_eq!(read.claim, "test claim");
    }

    #[test]
    fn id_collision_is_rejected() {
        let dir = tempfile::TempDir::new().unwrap();
        let lib = Library::init(dir.path()).unwrap();
        let created = chrono::Utc::now();
        let id = Engram::generate_id("shared claim", &created);

        let e1 = Engram {
            id: id.clone(),
            tier: Tier::Semantic,
            status: Status::Confirmed,
            claim: "first claim".into(),
            created,
            updated: created,
            last_touched: created,
            source: vec![],
            confidence: 0.9,
            salience: 0.7,
            collections: vec![],
            tags: vec![],
            links: vec![],
            embedding_ref: None,
            shape_ref: None,
            surface_when: None,
            output_policy: None,
            body: String::new(),
        };
        lib.write_engram(&e1).unwrap();

        let e2 = Engram {
            claim: "different claim".into(),
            ..e1
        };
        let err = lib.write_engram(&e2).unwrap_err();
        assert!(matches!(err, AlexandriaError::IdCollision { .. }));
    }

    #[test]
    fn scan_reports_parse_failures() {
        let dir = tempfile::TempDir::new().unwrap();
        let lib = Library::init(dir.path()).unwrap();
        fs::write(
            lib.root.join("semantic/bad.md"),
            "not valid frontmatter\n",
        )
        .unwrap();

        let scan = lib.scan_engrams();
        assert_eq!(scan.engrams.len(), 0);
        assert_eq!(scan.failures.len(), 1);
    }
}
