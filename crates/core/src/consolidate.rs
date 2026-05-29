use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

use chrono::{Duration, Utc};
use serde::Serialize;

use crate::config::{Config, ConsolidationConfig};
use crate::engram::{Engram, Link, Rel, Status, Tier};
use crate::error::Result;
use crate::graph::{has_conflicts_confirmed, incoming_supports_count};
use crate::index::Index;
use crate::meta::record_promotion_reversal;
use crate::ops::Ops;
use crate::provider::Completer;
use crate::shape::extract_shape_summary;
use crate::store::Library;

#[derive(Debug, Clone, Serialize, Default)]
pub struct ConsolidationReport {
    pub merged: Vec<String>,
    pub promoted: Vec<String>,
    pub demoted: Vec<String>,
    pub decayed: Vec<String>,
    pub collections_resummarized: Vec<String>,
    pub shapes_extracted: Vec<String>,
    pub relational_decayed: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FastReflectionReport {
    pub briefing_path: String,
    pub engrams_summarized: usize,
}

pub fn consolidate_slow(
    library: &Library,
    index: &Index,
    config: &Config,
    completer: Option<&dyn Completer>,
) -> Result<ConsolidationReport> {
    let mut report = ConsolidationReport::default();
    let ops = Ops::new(library, index);
    let cfg = &config.consolidation;

    dedupe_merge(library, index, &ops, cfg, &mut report)?;
    apply_promotion_ladder(library, index, &ops, cfg, &mut report)?;
    extract_shapes(library, index, completer, &mut report)?;
    decay_salience(library, index, cfg, &mut report)?;
    decay_relational_salience(library, index, &config.relational, &mut report)?;
    consolidate_relational(library, index, &config.relational, &mut report)?;
    resummarize_collections(library, &mut report)?;

    Ok(report)
}

/// Fast-pass reflection: non-canonical briefing material (ARCHITECTURE §6.1).
pub fn consolidate_fast(library: &Library, config: &Config) -> Result<FastReflectionReport> {
    let scan = library.scan_engrams();
    let cutoff = Utc::now() - Duration::hours(24);
    let recent: Vec<&Engram> = scan
        .engrams
        .iter()
        .filter(|e| e.tier == Tier::Episodic && e.last_touched >= cutoff)
        .collect();

    let dir = library.alexandria_dir().join("fast_reflections");
    fs::create_dir_all(&dir)?;
    let stamp = Utc::now().format("%Y-%m-%dT%H%M%S");
    let path = dir.join(format!("{stamp}.brief.md"));

    let mut claims: Vec<String> = recent
        .iter()
        .map(|e| format!("- [{}] {}", e.id, e.claim))
        .collect();
    claims.sort();

    let body = if claims.is_empty() {
        "No recent episodic engrams to summarize.".to_string()
    } else {
        claims.join("\n")
    };

    let content = format!(
        "---\ntrack: fast\nstatus: provisional\ncreated: {}\n---\n\n# Fast briefing\n\n{body}\n",
        Utc::now().to_rfc3339()
    );
    fs::write(&path, content)?;

    let _ = config;
    Ok(FastReflectionReport {
        briefing_path: path.display().to_string(),
        engrams_summarized: recent.len(),
    })
}

fn dedupe_merge(
    library: &Library,
    index: &Index,
    ops: &Ops,
    cfg: &ConsolidationConfig,
    report: &mut ConsolidationReport,
) -> Result<()> {
    let scan = library.scan_engrams();
    let mut active: Vec<Engram> = scan
        .engrams
        .into_iter()
        .filter(|e| e.status != Status::Superseded && e.status != Status::Archived)
        .collect();

    let mut merged_ids: HashSet<String> = HashSet::new();

    for i in 0..active.len() {
        if merged_ids.contains(&active[i].id) {
            continue;
        }
        let text = embed_text(&active[i]);
        let query_vec = index.embed_query(&text)?;
        let neighbors = index.semantic_knn(&query_vec, 20)?;

        for neighbor in neighbors {
            if neighbor.id == active[i].id || merged_ids.contains(&neighbor.id) {
                continue;
            }
            if (neighbor.distance as f32) > cfg.dedupe_max_distance {
                continue;
            }
            let Some(j) = active.iter().position(|e| e.id == neighbor.id) else {
                continue;
            };
            if merged_ids.contains(&active[j].id) {
                continue;
            }
            let overlap = claim_overlap(&active[i].claim, &active[j].claim);
            if overlap < cfg.dedupe_claim_overlap {
                continue;
            }

            let (survivor_local, _loser_local) = pick_survivor(&active[i], &active[j]);
            let (survivor_idx, loser_idx) = if survivor_local == 0 {
                (i, j)
            } else {
                (j, i)
            };
            let survivor = active[survivor_idx].clone();
            let mut loser = active[loser_idx].clone();

            let mut merged = survivor;
            merge_metadata(&mut merged, &loser);
            merged.updated = Utc::now();

            if !merged.links.iter().any(|l| l.rel == Rel::Supersedes && l.to == loser.id) {
                merged.links.push(Link {
                    rel: Rel::Supersedes,
                    to: loser.id.clone(),
                });
            }

            loser.status = Status::Superseded;
            loser.updated = Utc::now();
            if !loser.links.iter().any(|l| l.rel == Rel::SupersededBy && l.to == merged.id) {
                loser.links.push(Link {
                    rel: Rel::SupersededBy,
                    to: merged.id.clone(),
                });
            }

            persist_engram(library, index, &merged)?;
            persist_engram(library, index, &loser)?;

            merged_ids.insert(loser.id.clone());
            active[survivor_idx] = merged;
            report.merged.push(format!("{} -> {}", loser.id, active[survivor_idx].id));
        }
    }

    let _ = ops;
    Ok(())
}

fn apply_promotion_ladder(
    library: &Library,
    index: &Index,
    _ops: &Ops,
    cfg: &ConsolidationConfig,
    report: &mut ConsolidationReport,
) -> Result<()> {
    let scan = library.scan_engrams();
    for mut engram in scan.engrams {
        if engram.status == Status::Superseded || engram.status == Status::Archived {
            continue;
        }
        if engram.status == Status::UnresolvedByDesign {
            continue;
        }

        let supports = incoming_supports_count(index, &engram.id)?;
        let conflict = has_conflicts_confirmed(index, &engram.id)?;

        let mut changed = false;

        if conflict
            && engram.status != Status::Provisional
            && engram.tier != Tier::Relational
        {
            engram.status = Status::Provisional;
            if engram.tier == Tier::Semantic {
                engram.tier = Tier::Provisional;
            }
            engram.updated = Utc::now();
            changed = true;
            report.demoted.push(engram.id.clone());
            let from_tier = if engram.tier == Tier::Semantic {
                "semantic"
            } else {
                "provisional"
            };
            record_promotion_reversal(library, &engram.id, from_tier)?;
            index.insert_promotion_reversal(&engram.id, from_tier, &Utc::now())?;
        } else if engram.tier == Tier::Episodic
            && engram.status != Status::Provisional
            && supports >= cfg.promote_episodic_to_provisional
        {
            engram.tier = Tier::Provisional;
            engram.status = Status::Provisional;
            engram.updated = Utc::now();
            changed = true;
            report.promoted.push(format!("{}: episodic->provisional", engram.id));
        } else if engram.tier == Tier::Provisional
            && supports >= cfg.promote_provisional_to_semantic
        {
            engram.tier = Tier::Semantic;
            engram.status = Status::Confirmed;
            engram.updated = Utc::now();
            changed = true;
            report.promoted.push(format!("{}: provisional->semantic", engram.id));
        }

        if changed {
            persist_engram(library, index, &engram)?;
        }
    }
    Ok(())
}

fn extract_shapes(
    library: &Library,
    index: &Index,
    completer: Option<&dyn Completer>,
    report: &mut ConsolidationReport,
) -> Result<()> {
    index.ensure_shapes_vec_table()?;
    let scan = library.scan_engrams();
    for mut engram in scan.engrams {
        if engram.tier != Tier::Episodic {
            continue;
        }
        if engram.status == Status::Superseded || engram.status == Status::Archived {
            continue;
        }
        let summary = extract_shape_summary(&engram, completer)?;
        let shape_id = format!("shp_{}", &engram.id[4..]);
        engram.shape_ref = Some(shape_id);
        engram.updated = Utc::now();
        persist_engram(library, index, &engram)?;
        index.upsert_shape_embedding(&engram.id, &summary)?;
        report.shapes_extracted.push(engram.id.clone());
    }
    Ok(())
}

fn decay_relational_salience(
    library: &Library,
    index: &Index,
    cfg: &crate::config::RelationalConfig,
    report: &mut ConsolidationReport,
) -> Result<()> {
    let now = Utc::now();
    let scan = library.scan_engrams();
    for mut engram in scan.engrams {
        if engram.tier != Tier::Relational {
            continue;
        }
        if engram.status == Status::Superseded || engram.status == Status::Archived {
            continue;
        }
        let elapsed_days = (now - engram.last_touched).num_seconds().max(0) as f64 / 86400.0;
        if elapsed_days <= 0.0 {
            continue;
        }
        let decay_factor = 0.5f64.powf(elapsed_days / cfg.salience_half_life_days);
        let new_salience = (engram.salience * decay_factor).max(0.05);
        if (new_salience - engram.salience).abs() < f64::EPSILON {
            continue;
        }
        engram.salience = new_salience;
        engram.updated = Utc::now();
        persist_engram(library, index, &engram)?;
        report.relational_decayed.push(engram.id.clone());
    }
    Ok(())
}

fn consolidate_relational(
    library: &Library,
    index: &Index,
    cfg: &crate::config::RelationalConfig,
    report: &mut ConsolidationReport,
) -> Result<()> {
    let scan = library.scan_engrams();
    for mut engram in scan.engrams {
        if engram.tier != Tier::Relational {
            continue;
        }
        if engram.status == Status::Confirmed {
            continue;
        }
        let evidence = parse_relational_evidence(&engram);
        if evidence.projects >= cfg.min_projects
            && evidence.task_types >= cfg.min_task_types
            && evidence.registers >= cfg.min_registers
        {
            engram.status = Status::Confirmed;
            engram.updated = Utc::now();
            persist_engram(library, index, &engram)?;
            report.promoted.push(format!("{}: relational->confirmed", engram.id));
        }
    }
    Ok(())
}

struct RelationalEvidence {
    projects: u32,
    task_types: u32,
    registers: u32,
}

fn parse_relational_evidence(engram: &Engram) -> RelationalEvidence {
    let mut ev = RelationalEvidence {
        projects: 0,
        task_types: 0,
        registers: 0,
    };
    for tag in &engram.tags {
        if let Some(rest) = tag.strip_prefix("evidence:") {
            for part in rest.split(',') {
                let part = part.trim();
                if let Some(n) = part.strip_prefix("projects=").and_then(|s| s.parse().ok()) {
                    ev.projects = ev.projects.max(n);
                } else if let Some(n) =
                    part.strip_prefix("task_types=").and_then(|s| s.parse().ok())
                {
                    ev.task_types = ev.task_types.max(n);
                } else if let Some(n) = part.strip_prefix("registers=").and_then(|s| s.parse().ok())
                {
                    ev.registers = ev.registers.max(n);
                }
            }
        }
    }
    if ev.projects == 0 && !engram.collections.is_empty() {
        ev.projects = engram.collections.len() as u32;
    }
    if ev.task_types == 0 && !engram.tags.is_empty() {
        ev.task_types = 1;
    }
    if ev.registers == 0 {
        ev.registers = 1;
    }
    ev
}

fn decay_salience(
    library: &Library,
    index: &Index,
    cfg: &ConsolidationConfig,
    report: &mut ConsolidationReport,
) -> Result<()> {
    let now = Utc::now();
    let scan = library.scan_engrams();
    for mut engram in scan.engrams {
        if engram.status == Status::Superseded || engram.status == Status::Archived {
            continue;
        }
        let elapsed_days = (now - engram.last_touched).num_seconds().max(0) as f64 / 86400.0;
        if elapsed_days <= 0.0 {
            continue;
        }
        let decay_factor = 0.5f64.powf(elapsed_days / cfg.salience_half_life_days);
        let new_salience = (engram.salience * decay_factor).max(cfg.salience_floor);
        if (new_salience - engram.salience).abs() < f64::EPSILON {
            continue;
        }
        engram.salience = new_salience;
        engram.updated = Utc::now();
        persist_engram(library, index, &engram)?;
        report.decayed.push(engram.id.clone());
    }
    Ok(())
}

fn resummarize_collections(library: &Library, report: &mut ConsolidationReport) -> Result<()> {
    let scan = library.scan_engrams();
    let mut by_collection: HashMap<String, Vec<&Engram>> = HashMap::new();
    for engram in &scan.engrams {
        if engram.status == Status::Superseded || engram.status == Status::Archived {
            continue;
        }
        for collection in &engram.collections {
            by_collection.entry(collection.clone()).or_default().push(engram);
        }
    }

    let collections_dir = library.root.join("collections");
    std::fs::create_dir_all(&collections_dir)?;

    for (name, members) in by_collection {
        let mut claims: Vec<String> = members.iter().map(|e| format!("- [{}] {}", e.id, e.claim)).collect();
        claims.sort();
        let summary = format!(
            "Collection `{}` — {} engram(s)\n\n{}",
            name,
            members.len(),
            claims.join("\n")
        );
        let slug = name.replace('/', "-");
        let path = collections_dir.join(format!("{slug}.md"));
        let content = format!(
            "---\ncollection: {name}\nupdated: {}\nmember_count: {}\n---\n\n{summary}\n",
            Utc::now().to_rfc3339(),
            members.len()
        );
        std::fs::write(&path, content)?;
        report.collections_resummarized.push(name);
    }

    Ok(())
}

fn persist_engram(library: &Library, index: &Index, engram: &Engram) -> Result<()> {
    let old_path = index.file_path(&engram.id)?;
    let path = library.save_relocating(engram, old_path.as_deref().map(Path::new))?;
    index.upsert(engram, &path.display().to_string())?;
    Ok(())
}

fn embed_text(engram: &Engram) -> String {
    if engram.body.trim().is_empty() {
        engram.claim.clone()
    } else {
        format!("{}\n{}", engram.claim, engram.body)
    }
}

fn claim_overlap(a: &str, b: &str) -> f64 {
    let ta: HashSet<_> = a
        .split_whitespace()
        .map(|s| s.to_lowercase())
        .collect();
    let tb: HashSet<_> = b
        .split_whitespace()
        .map(|s| s.to_lowercase())
        .collect();
    if ta.is_empty() || tb.is_empty() {
        return 0.0;
    }
    let inter = ta.intersection(&tb).count() as f64;
    let union = ta.union(&tb).count() as f64;
    inter / union
}

fn pick_survivor(a: &Engram, b: &Engram) -> (usize, usize) {
    // Returns indices relative to (a, b) as 0 and 1
    if a.confidence > b.confidence {
        return (0, 1);
    }
    if b.confidence > a.confidence {
        return (1, 0);
    }
    if a.links.len() > b.links.len() {
        return (0, 1);
    }
    if b.links.len() > a.links.len() {
        return (1, 0);
    }
    if a.created <= b.created {
        (0, 1)
    } else {
        (1, 0)
    }
}

fn merge_metadata(survivor: &mut Engram, loser: &Engram) {
    for c in &loser.collections {
        if !survivor.collections.contains(c) {
            survivor.collections.push(c.clone());
        }
    }
    for t in &loser.tags {
        if !survivor.tags.contains(t) {
            survivor.tags.push(t.clone());
        }
    }
    for s in &loser.source {
        if !survivor.source.contains(s) {
            survivor.source.push(s.clone());
        }
    }
    for link in &loser.links {
        if !survivor
            .links
            .iter()
            .any(|l| l.rel == link.rel && l.to == link.to)
        {
            survivor.links.push(link.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::build_embedder;
    use chrono::Duration;
    use tempfile::TempDir;

    fn setup() -> (TempDir, Library, Index, Config) {
        let dir = TempDir::new().unwrap();
        let lib = Library::init(dir.path()).unwrap();
        let mut config = Config::load(dir.path()).unwrap();
        config.providers.embedder = "hash".into();
        config.consolidation.dedupe_max_distance = 1.5;
        config.consolidation.dedupe_claim_overlap = 0.5;
        let embedder = build_embedder(&config).unwrap();
        let index = Index::open_with_embedder(&lib, embedder).unwrap();
        (dir, lib, index, config)
    }

    fn remember(lib: &Library, index: &Index, engram: &Engram) {
        let p = lib.write_engram(engram).unwrap();
        index.upsert(engram, &p.display().to_string()).unwrap();
    }

    #[test]
    fn dedupe_merges_near_duplicates() {
        let (_dir, lib, index, config) = setup();
        let a = Engram::new(
            "Alexandria uses hybrid fused retrieval",
            "Vector-only retrieval fails.",
            Tier::Semantic,
            Status::Confirmed,
        );
        let b = Engram::new(
            "Alexandria uses hybrid retrieval fusion",
            "Different body.",
            Tier::Semantic,
            Status::Confirmed,
        );
        remember(&lib, &index, &a);
        remember(&lib, &index, &b);

        let report = consolidate_slow(&lib, &index, &config, None).unwrap();
        assert!(!report.merged.is_empty());

        let a_state = lib.read_engram(&lib.engram_path(&a).unwrap());
        let b_state = lib.read_engram(&lib.engram_path(&b).unwrap());
        let a_archived = lib.read_engram(&lib.root.join(format!("archive/{}.md", a.id)));
        let b_archived = lib.read_engram(&lib.root.join(format!("archive/{}.md", b.id)));

        let superseded = [a_state, b_state, a_archived, b_archived]
            .into_iter()
            .filter_map(Result::ok)
            .find(|e| e.status == Status::Superseded);
        assert!(superseded.is_some(), "expected one superseded engram");
    }

    #[test]
    fn promotion_episodic_to_provisional_to_semantic() {
        let (_dir, lib, index, mut config) = setup();
        config.consolidation.promote_episodic_to_provisional = 1;
        config.consolidation.promote_provisional_to_semantic = 2;

        let episodic = Engram::new("event happened", "body", Tier::Episodic, Status::Confirmed);
        let mut supporter1 = Engram::new("support 1", "b", Tier::Semantic, Status::Confirmed);
        let mut supporter2 = Engram::new("support 2", "b", Tier::Semantic, Status::Confirmed);
        supporter1.links.push(Link {
            rel: Rel::Supports,
            to: episodic.id.clone(),
        });
        supporter2.links.push(Link {
            rel: Rel::Supports,
            to: episodic.id.clone(),
        });

        remember(&lib, &index, &episodic);
        remember(&lib, &index, &supporter1);
        remember(&lib, &index, &supporter2);

        let report = consolidate_slow(&lib, &index, &config, None).unwrap();
        assert!(report.promoted.iter().any(|p| p.contains("episodic->provisional")));

        let report2 = consolidate_slow(&lib, &index, &config, None).unwrap();
        assert!(report2.promoted.iter().any(|p| p.contains("provisional->semantic"))
            || report.promoted.iter().any(|p| p.contains("provisional->semantic")));
    }

    #[test]
    fn decay_reduces_salience() {
        let (_dir, lib, index, config) = setup();
        let mut e = Engram::new("old memory", "body", Tier::Semantic, Status::Confirmed);
        e.last_touched = Utc::now() - Duration::days(60);
        e.salience = 0.8;
        remember(&lib, &index, &e);

        let report = consolidate_slow(&lib, &index, &config, None).unwrap();
        assert!(report.decayed.contains(&e.id));
        let updated = lib.read_engram(&lib.engram_path(&e).unwrap()).unwrap();
        assert!(updated.salience < 0.8);
        assert!(updated.salience >= config.consolidation.salience_floor);
    }

    #[test]
    fn consolidate_is_idempotent() {
        let (_dir, lib, index, config) = setup();
        let mut e = Engram::new(
            "stable fact",
            "body",
            Tier::Semantic,
            Status::Confirmed,
        );
        e.collections.push("demo".into());
        remember(&lib, &index, &e);

        let first = consolidate_slow(&lib, &index, &config, None).unwrap();
        let second = consolidate_slow(&lib, &index, &config, None).unwrap();
        assert_eq!(first.merged.len(), second.merged.len());
        assert_eq!(first.promoted.len(), second.promoted.len());
        assert_eq!(first.demoted.len(), second.demoted.len());
    }

    #[test]
    fn collection_rollups_written() {
        let (_dir, lib, index, config) = setup();
        let mut e = Engram::new("in collection", "body", Tier::Semantic, Status::Confirmed);
        e.collections.push("demo/project".into());
        remember(&lib, &index, &e);

        let report = consolidate_slow(&lib, &index, &config, None).unwrap();
        assert!(report.collections_resummarized.contains(&"demo/project".to_string()));
        assert!(lib.root.join("collections/demo-project.md").exists());
    }
}
