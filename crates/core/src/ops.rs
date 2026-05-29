use std::path::Path;

use chrono::Utc;
use serde::Serialize;

use crate::engram::{Engram, Link, Rel, Status};
use crate::error::{AlexandriaError, Result};
use crate::index::Index;
use crate::store::Library;

#[derive(Debug, Clone, Serialize)]
pub struct LinkResult {
    pub from_id: String,
    pub rel: String,
    pub to_id: String,
    pub reciprocal_added: bool,
    pub target_superseded: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ArchiveResult {
    pub id: String,
    pub claim: String,
    pub status: String,
    pub path: String,
}

pub struct Ops<'a> {
    library: &'a Library,
    index: &'a Index,
}

impl<'a> Ops<'a> {
    pub fn new(library: &'a Library, index: &'a Index) -> Self {
        Self { library, index }
    }

    pub fn link(&self, from_id: &str, rel: Rel, to_id: &str) -> Result<LinkResult> {
        if from_id == to_id {
            return Err(AlexandriaError::InvalidEngram(
                "cannot link an engram to itself".into(),
            ));
        }
        let mut from = self.load_engram(from_id)?;
        let mut to = self.load_engram(to_id)?;

        if !from.links.iter().any(|l| l.rel == rel && l.to == to_id) {
            from.links.push(Link {
                rel,
                to: to_id.to_string(),
            });
        }

        let mut reciprocal_added = false;
        let mut target_superseded = false;

        if let Some(reciprocal) = rel.reciprocal() {
            if !to.links.iter().any(|l| l.rel == reciprocal && l.to == from_id) {
                to.links.push(Link {
                    rel: reciprocal,
                    to: from_id.to_string(),
                });
                reciprocal_added = true;
            }
        }

        if rel == Rel::Supersedes {
            to.status = Status::Superseded;
            to.updated = Utc::now();
            target_superseded = true;
        }

        from.updated = Utc::now();
        to.updated = Utc::now();

        let from_old = self.index.file_path(from_id)?;
        let to_old = self.index.file_path(to_id)?;

        let from_path = self.library.save_relocating(
            &from,
            from_old.as_deref().map(Path::new),
        )?;
        let to_path = self.library.save_relocating(
            &to,
            to_old.as_deref().map(Path::new),
        )?;

        self.index
            .upsert(&from, &from_path.display().to_string())?;
        self.index.upsert(&to, &to_path.display().to_string())?;

        Ok(LinkResult {
            from_id: from_id.to_string(),
            rel: rel.as_str().to_string(),
            to_id: to_id.to_string(),
            reciprocal_added,
            target_superseded,
        })
    }

    pub fn archive(&self, id: &str) -> Result<ArchiveResult> {
        self.set_status(id, Status::Archived)
    }

    pub fn forget(&self, id: &str) -> Result<ArchiveResult> {
        self.set_status(id, Status::Archived)
    }

    fn set_status(&self, id: &str, status: Status) -> Result<ArchiveResult> {
        let mut engram = self.load_engram(id)?;
        engram.status = status;
        engram.updated = Utc::now();
        let old_path = self.index.file_path(id)?;
        let path = self
            .library
            .save_relocating(&engram, old_path.as_deref().map(Path::new))?;
        self.index
            .upsert(&engram, &path.display().to_string())?;
        Ok(ArchiveResult {
            id: engram.id,
            claim: engram.claim,
            status: status_label(engram.status).to_string(),
            path: path.display().to_string(),
        })
    }

    fn load_engram(&self, id: &str) -> Result<Engram> {
        let path = self
            .index
            .file_path(id)?
            .ok_or_else(|| AlexandriaError::EngramNotFound(id.to_string()))?;
        self.library.read_engram(Path::new(&path))
    }
}

fn status_label(status: Status) -> &'static str {
    match status {
        Status::Confirmed => "confirmed",
        Status::Provisional => "provisional",
        Status::UnresolvedByDesign => "unresolved_by_design",
        Status::Superseded => "superseded",
        Status::Archived => "archived",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engram::Tier;
    use crate::provider::build_embedder;
    use crate::{Config, Index, Library};

    fn setup() -> (tempfile::TempDir, Library, Index, Config) {
        let dir = tempfile::TempDir::new().unwrap();
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
    fn link_adds_symmetric_conflict_edge() {
        let (_dir, lib, index, _config) = setup();
        let a = Engram::new("claim a", "b", Tier::Semantic, Status::Confirmed);
        let b = Engram::new("claim b", "b", Tier::Semantic, Status::Confirmed);
        remember(&lib, &index, &a);
        remember(&lib, &index, &b);

        let ops = Ops::new(&lib, &index);
        let result = ops
            .link(&a.id, Rel::ConflictsConfirmed, &b.id)
            .unwrap();
        assert!(result.reciprocal_added);

        let updated_b = ops.load_engram(&b.id).unwrap();
        assert!(updated_b
            .links
            .iter()
            .any(|l| l.rel == Rel::ConflictsConfirmed && l.to == a.id));
    }

    #[test]
    fn supersedes_relocates_target_to_archive() {
        let (_dir, lib, index, _config) = setup();
        let old = Engram::new("old claim", "b", Tier::Semantic, Status::Confirmed);
        let new = Engram::new("new claim", "b", Tier::Semantic, Status::Confirmed);
        let old_path = lib.write_engram(&old).unwrap();
        remember(&lib, &index, &new);
        index.upsert(&old, &old_path.display().to_string()).unwrap();

        let ops = Ops::new(&lib, &index);
        let result = ops.link(&new.id, Rel::Supersedes, &old.id).unwrap();
        assert!(result.target_superseded);

        let updated = ops.load_engram(&old.id).unwrap();
        assert_eq!(updated.status, Status::Superseded);
        assert!(lib.engram_path(&updated).unwrap().starts_with(lib.root.join("archive")));
        assert!(!old_path.exists());
    }

    #[test]
    fn archive_relocates_file() {
        let (_dir, lib, index, _config) = setup();
        let e = Engram::new("to archive", "b", Tier::Semantic, Status::Confirmed);
        let old_path = lib.write_engram(&e).unwrap();
        index.upsert(&e, &old_path.display().to_string()).unwrap();

        let ops = Ops::new(&lib, &index);
        let result = ops.archive(&e.id).unwrap();
        assert_eq!(result.status, "archived");
        assert!(!old_path.exists());
        assert!(Path::new(&result.path).exists());
    }
}
