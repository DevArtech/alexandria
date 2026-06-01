//! Curated, snapshotted export of a memory slice as a portable read-only pack.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::engram::{Engram, Rel, Status, Tier};
use crate::error::{AlexandriaError, Result};
use crate::index::Index;
use crate::provider::predict_embedder_id;
use crate::store::Library;

pub const PACK_VERSION: &str = "1";

#[derive(Debug, Clone)]
pub struct PackExportOptions {
    pub target: PathBuf,
    pub name: Option<String>,
    pub collections: Vec<String>,
    pub tags: Vec<String>,
    pub include_archived: bool,
    pub alexandria_version: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PackReport {
    pub path: String,
    pub name: String,
    pub exported_at: DateTime<Utc>,
    pub engram_count: usize,
    pub skipped_relational: usize,
    pub skipped_archived: usize,
    pub skipped_no_match: usize,
    pub collections: Vec<String>,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
struct PackManifest {
    pack_version: String,
    name: String,
    snapshot: bool,
    source_library: String,
    exported_at: DateTime<Utc>,
    alexandria_version: String,
    embedder_id: Option<String>,
    selector: PackSelector,
    counts: PackCounts,
}

#[derive(Debug, Clone, Serialize)]
struct PackSelector {
    collections: Vec<String>,
    tags: Vec<String>,
    include_archived: bool,
}

#[derive(Debug, Clone, Serialize)]
struct PackCounts {
    engrams: usize,
    skipped_relational: usize,
    skipped_archived: usize,
    skipped_no_match: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct IndexEntry {
    id: String,
    claim: String,
    tier: Tier,
    status: Status,
    confidence: f64,
    salience: f64,
    collections: Vec<String>,
    tags: Vec<String>,
    links: Vec<IndexLink>,
    article: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct IndexLink {
    rel: Rel,
    to: String,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    external: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
struct SourceKey {
    kind: String,
    r#ref: String,
}

#[derive(Debug, Clone, Serialize)]
struct PackSourceEntry {
    kind: String,
    r#ref: String,
    observed: Option<DateTime<Utc>>,
    engram_ids: Vec<String>,
}

/// Export a curated, read-only snapshot pack from the library.
pub fn export_pack(
    library: &Library,
    index: &Index,
    config: &Config,
    opts: &PackExportOptions,
) -> Result<PackReport> {
    if opts.collections.is_empty() && opts.tags.is_empty() {
        return Err(AlexandriaError::InvalidEngram(
            "pack export requires at least one --collection or --tag selector".into(),
        ));
    }

    let exported_at = Utc::now();
    let snapshot_of = library
        .root
        .canonicalize()
        .unwrap_or_else(|_| library.root.clone())
        .display()
        .to_string();

    let scan = library.scan_engrams();
    let mut skipped_relational = 0usize;
    let mut skipped_archived = 0usize;
    let mut skipped_no_match = 0usize;
    let mut selected: Vec<Engram> = Vec::new();

    for engram in scan.engrams {
        if engram.tier == Tier::Relational {
            skipped_relational += 1;
            continue;
        }
        if !opts.include_archived
            && (engram.status == Status::Archived || engram.status == Status::Superseded)
        {
            skipped_archived += 1;
            continue;
        }
        if !matches_selector(&engram, &opts.collections, &opts.tags) {
            skipped_no_match += 1;
            continue;
        }
        selected.push(engram);
    }

    selected.sort_by(|a, b| a.id.cmp(&b.id));
    let included_ids: HashSet<String> = selected.iter().map(|e| e.id.clone()).collect();

    let target = &opts.target;
    if target.exists() {
        let mut entries = fs::read_dir(target)?
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_name() != "." && e.file_name() != ".." && e.file_name() != ".DS_Store"
            })
            .peekable();
        if entries.peek().is_some() {
            return Err(AlexandriaError::InvalidEngram(format!(
                "pack export target is not empty: {}",
                target.display()
            )));
        }
    } else {
        fs::create_dir_all(target)?;
    }

    let articles_dir = target.join("articles");
    fs::create_dir_all(&articles_dir)?;

    let name = opts.name.clone().unwrap_or_else(|| {
        if !opts.collections.is_empty() {
            format!("Alexandria pack: {}", opts.collections.join(", "))
        } else {
            format!("Alexandria pack: tags {}", opts.tags.join(", "))
        }
    });

    let embedder_id = index
        .embedder_id()
        .ok()
        .or_else(|| predict_embedder_id(config));

    let mut sources_map: HashMap<SourceKey, PackSourceEntry> = HashMap::new();
    let mut index_entries: Vec<IndexEntry> = Vec::with_capacity(selected.len());

    for engram in &selected {
        let article_path = format!("articles/{}.md", engram.id);
        let article_abs = articles_dir.join(format!("{}.md", engram.id));
        let content = serialize_snapshot_article(engram, &snapshot_of, exported_at)?;
        fs::write(&article_abs, content)?;

        for source in &engram.source {
            let key = SourceKey {
                kind: source.kind.clone(),
                r#ref: source.r#ref.clone(),
            };
            sources_map
                .entry(key)
                .and_modify(|entry| {
                    if source.observed.is_some() {
                        entry.observed = merge_observed(entry.observed, source.observed);
                    }
                    if !entry.engram_ids.contains(&engram.id) {
                        entry.engram_ids.push(engram.id.clone());
                    }
                })
                .or_insert_with(|| PackSourceEntry {
                    kind: source.kind.clone(),
                    r#ref: source.r#ref.clone(),
                    observed: source.observed,
                    engram_ids: vec![engram.id.clone()],
                });
        }

        let links = engram
            .links
            .iter()
            .map(|link| IndexLink {
                rel: link.rel,
                to: link.to.clone(),
                external: !included_ids.contains(&link.to),
            })
            .collect();

        index_entries.push(IndexEntry {
            id: engram.id.clone(),
            claim: engram.claim.clone(),
            tier: engram.tier,
            status: engram.status,
            confidence: engram.confidence,
            salience: engram.salience,
            collections: engram.collections.clone(),
            tags: engram.tags.clone(),
            links,
            article: article_path,
        });
    }

    let mut sources: Vec<PackSourceEntry> = sources_map.into_values().collect();
    for source in &mut sources {
        source.engram_ids.sort();
    }
    sources.sort_by(|a, b| (&a.kind, &a.r#ref).cmp(&(&b.kind, &b.r#ref)));

    let manifest = PackManifest {
        pack_version: PACK_VERSION.into(),
        name: name.clone(),
        snapshot: true,
        source_library: snapshot_of.clone(),
        exported_at,
        alexandria_version: opts.alexandria_version.clone(),
        embedder_id,
        selector: PackSelector {
            collections: opts.collections.clone(),
            tags: opts.tags.clone(),
            include_archived: opts.include_archived,
        },
        counts: PackCounts {
            engrams: selected.len(),
            skipped_relational,
            skipped_archived,
            skipped_no_match,
        },
    };

    write_json(target.join("manifest.json"), &manifest)?;
    write_json(target.join("INDEX.json"), &index_entries)?;
    write_json(target.join("sources.json"), &sources)?;
    fs::write(
        target.join("AGENTS.md"),
        render_agents_md(&name, &snapshot_of, exported_at, &selected, opts),
    )?;

    Ok(PackReport {
        path: target.display().to_string(),
        name,
        exported_at,
        engram_count: selected.len(),
        skipped_relational,
        skipped_archived,
        skipped_no_match,
        collections: opts.collections.clone(),
        tags: opts.tags.clone(),
    })
}

fn matches_selector(engram: &Engram, collections: &[String], tags: &[String]) -> bool {
    let collection_match = collections
        .iter()
        .any(|c| engram.collections.iter().any(|ec| ec == c));
    let tag_match = tags.iter().any(|t| engram.tags.iter().any(|et| et == t));
    collection_match || tag_match
}

fn merge_observed(
    existing: Option<DateTime<Utc>>,
    incoming: Option<DateTime<Utc>>,
) -> Option<DateTime<Utc>> {
    match (existing, incoming) {
        (Some(a), Some(b)) => Some(if a > b { a } else { b }),
        (None, Some(b)) => Some(b),
        (Some(a), None) => Some(a),
        (None, None) => None,
    }
}

fn serialize_snapshot_article(
    engram: &Engram,
    snapshot_of: &str,
    exported_at: DateTime<Utc>,
) -> Result<String> {
    let yaml = serde_yaml::to_string(&engram.to_frontmatter())
        .map_err(|e| AlexandriaError::InvalidEngram(e.to_string()))?;
    let snapshot_block = format!(
        "snapshot: true\nsnapshot_of: {snapshot_of}\nexported: {}\n",
        exported_at.to_rfc3339()
    );
    let body = engram.body.trim();
    if body.is_empty() {
        Ok(format!("---\n{yaml}{snapshot_block}---\n"))
    } else {
        Ok(format!("---\n{yaml}{snapshot_block}---\n\n{body}\n"))
    }
}

fn write_json<T: Serialize>(path: PathBuf, value: &T) -> Result<()> {
    let json = serde_json::to_string_pretty(value)
        .map_err(|e| AlexandriaError::InvalidEngram(e.to_string()))?;
    fs::write(path, json)?;
    Ok(())
}

fn render_agents_md(
    name: &str,
    snapshot_of: &str,
    exported_at: DateTime<Utc>,
    engrams: &[Engram],
    opts: &PackExportOptions,
) -> String {
    let mut collections: HashSet<String> = HashSet::new();
    let mut tags: HashSet<String> = HashSet::new();
    for engram in engrams {
        for c in &engram.collections {
            collections.insert(c.clone());
        }
        for t in &engram.tags {
            tags.insert(t.clone());
        }
    }
    let mut collections: Vec<_> = collections.into_iter().collect();
    collections.sort();
    let mut tags: Vec<_> = tags.into_iter().collect();
    tags.sort();

    let selector_lines = {
        let mut lines = Vec::new();
        if !opts.collections.is_empty() {
            lines.push(format!("- collections: {}", opts.collections.join(", ")));
        }
        if !opts.tags.is_empty() {
            lines.push(format!("- tags: {}", opts.tags.join(", ")));
        }
        if opts.include_archived {
            lines.push("- include_archived: true".into());
        }
        lines.join("\n")
    };

    format!(
        r#"# {name}

> **Read-only snapshot** exported from Alexandria on {exported_at}. This pack is a frozen reference layer — not live memory. Do not treat it as writable or authoritative beyond its export timestamp.

## What this is

This directory is an Alexandria memory pack: a curated slice of structured knowledge where each file in `articles/` is one **engram** (a single claim with provenance, confidence, and typed links).

- Source library: `{snapshot_of}`
- Engrams included: {engram_count}
- Pack format version: {pack_version}

## How to navigate

1. Read `manifest.json` for export metadata and selector details.
2. Scan `INDEX.json` for all engram ids, claims, tiers, and links (`external: true` means the target is outside this pack).
3. Open `articles/<id>.md` for full claim + body + frontmatter.
4. Consult `sources.json` for deduplicated provenance across engrams.

## Selector used

{selector_lines}

## Collections in this pack

{collections_section}

## Tags in this pack

{tags_section}

## Conventions

- **Atomic unit:** one engram = one claim with provenance — not a synthesized concept article.
- **Typed edges:** links use explicit relations (`supports`, `conflicts_confirmed`, `depends_on`, etc.), not wiki-style cross-references.
- **Relational memory excluded:** user-preference / generation-only memory is never exported.
- **Snapshot only:** content may be stale relative to the source library; prefer the live library when available.
"#,
        name = name,
        exported_at = exported_at.to_rfc3339(),
        snapshot_of = snapshot_of,
        engram_count = engrams.len(),
        pack_version = PACK_VERSION,
        selector_lines = selector_lines,
        collections_section = if collections.is_empty() {
            "_None_".into()
        } else {
            collections
                .iter()
                .map(|c| format!("- `{c}`"))
                .collect::<Vec<_>>()
                .join("\n")
        },
        tags_section = if tags.is_empty() {
            "_None_".into()
        } else {
            tags.iter()
                .map(|t| format!("- `{t}`"))
                .collect::<Vec<_>>()
                .join("\n")
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engram::{Link, Rel, Source, Status, Tier};
    use crate::provider::build_embedder;
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
    fn export_pack_writes_layout_and_excludes_relational() {
        let (_dir, lib, index, config) = setup();
        let target = _dir.path().join("out-pack");

        let mut semantic = Engram::new(
            "Cartographer uses hybrid retrieval",
            "Details here.",
            Tier::Semantic,
            Status::Confirmed,
        );
        semantic.collections.push("cartographer".into());
        semantic.tags.push("retrieval".into());
        semantic
            .source
            .push(Source::parse_cli("repo:alexandria").unwrap());

        let mut relational = Engram::new(
            "User prefers terse answers",
            "",
            Tier::Relational,
            Status::Confirmed,
        );
        relational.collections.push("cartographer".into());

        remember(&lib, &index, &semantic);
        remember(&lib, &index, &relational);

        let report = export_pack(
            &lib,
            &index,
            &config,
            &PackExportOptions {
                target: target.clone(),
                name: Some("Cartographer".into()),
                collections: vec!["cartographer".into()],
                tags: vec![],
                include_archived: false,
                alexandria_version: "0.1.0".into(),
            },
        )
        .unwrap();

        assert_eq!(report.engram_count, 1);
        assert_eq!(report.skipped_relational, 1);
        assert!(target.join("AGENTS.md").exists());
        assert!(target.join("manifest.json").exists());
        assert!(target.join("INDEX.json").exists());
        assert!(target.join("sources.json").exists());
        assert!(target.join(format!("articles/{}.md", semantic.id)).exists());

        let article =
            fs::read_to_string(target.join(format!("articles/{}.md", semantic.id))).unwrap();
        assert!(article.contains("snapshot: true"));
        assert!(article.contains("snapshot_of:"));
    }

    #[test]
    fn export_pack_marks_external_links() {
        let (_dir, lib, index, config) = setup();
        let target = _dir.path().join("link-pack");

        let mut a = Engram::new("claim a", "body", Tier::Semantic, Status::Confirmed);
        a.collections.push("demo".into());
        remember(&lib, &index, &a);

        let mut b = Engram::new("claim b", "body", Tier::Semantic, Status::Confirmed);
        b.collections.push("other".into());
        b.links.push(Link {
            rel: Rel::Supports,
            to: a.id.clone(),
        });
        remember(&lib, &index, &b);

        export_pack(
            &lib,
            &index,
            &config,
            &PackExportOptions {
                target: target.clone(),
                name: None,
                collections: vec!["other".into()],
                tags: vec![],
                include_archived: false,
                alexandria_version: "0.1.0".into(),
            },
        )
        .unwrap();

        let index_json: Vec<IndexEntry> =
            serde_json::from_str(&fs::read_to_string(target.join("INDEX.json")).unwrap()).unwrap();
        assert_eq!(index_json.len(), 1);
        assert_eq!(index_json[0].links.len(), 1);
        assert!(index_json[0].links[0].external);
    }

    #[test]
    fn export_requires_selector() {
        let (_dir, lib, index, config) = setup();
        let err = export_pack(
            &lib,
            &index,
            &config,
            &PackExportOptions {
                target: _dir.path().join("empty-pack"),
                name: None,
                collections: vec![],
                tags: vec![],
                include_archived: false,
                alexandria_version: "0.1.0".into(),
            },
        )
        .unwrap_err();
        assert!(matches!(err, AlexandriaError::InvalidEngram(_)));
    }
}
