use std::io::{self, Read};
use std::path::PathBuf;

use alexandria_core::{Engram, Index, Library, Source, Status, Tier};
use anyhow::Result;

use crate::OutputFormat;

pub struct RememberOptions {
    pub library_path: Option<PathBuf>,
    pub format: OutputFormat,
    pub text: String,
    pub tier: Option<String>,
    pub status: Option<String>,
    pub collections: Vec<String>,
    pub tags: Vec<String>,
    pub sources: Vec<String>,
    pub derived_from: Vec<String>,
    /// Surfacing triggers for open threads (repeatable), e.g. topic:pricing
    pub surface_when: Vec<String>,
    /// ISO8601 observation time for --source entries
    pub observed: Option<String>,
}

pub fn run(opts: RememberOptions) -> Result<()> {
    let content = if opts.text == "-" {
        let mut buf = String::new();
        io::stdin().read_to_string(&mut buf)?;
        buf
    } else {
        opts.text
    };

    let (claim, body) = split_claim_body(&content);

    let tier = match opts.tier.as_deref() {
        Some(s) => Tier::parse(s)?,
        None => Tier::Semantic,
    };

    let status = match opts.status.as_deref() {
        Some(s) => Status::parse(s)?,
        None => Status::Confirmed,
    };

    let library = match opts.library_path {
        Some(p) => Library::discover(Some(&p))?,
        None => Library::discover(None)?,
    };

    let mut engram = Engram::new(claim, body, tier, status);
    engram.collections = opts.collections;
    engram.tags = opts.tags;

    for s in opts.sources {
        let mut source = Source::parse_cli(&s)?;
        source.resolve_observed(opts.observed.as_deref())?;
        engram.source.push(source);
    }
    for id in opts.derived_from {
        engram.source.push(Source::derived_from(&id));
    }
    if !opts.surface_when.is_empty() {
        engram.surface_when = Some(opts.surface_when);
    }

    let path = library.write_engram(&engram)?;
    let config = alexandria_core::Config::load(&library.root)?;
    let index = Index::open(&library, &config)?;
    index.upsert(&engram, &path.display().to_string())?;

    match opts.format {
        OutputFormat::Human => {
            println!("Remembered {} ({})", engram.id, engram.claim);
            println!("  tier: {:?}", engram.tier);
            println!("  path: {}", path.display());
            if !engram.source.is_empty() {
                println!("  sources: {}", engram.source.len());
            }
            if let Some(triggers) = &engram.surface_when {
                println!("  surface_when: {}", triggers.join(", "));
            }
        }
        OutputFormat::Json => {
            println!(
                "{}",
                serde_json::json!({
                    "id": engram.id,
                    "claim": engram.claim,
                    "tier": tier_label(engram.tier),
                    "status": status_label(engram.status),
                    "path": path.display().to_string(),
                    "sources": engram.source,
                    "surface_when": engram.surface_when,
                    "token_cost": Engram::estimate_tokens(&engram.claim)
                })
            );
        }
    }
    Ok(())
}

fn tier_label(tier: Tier) -> &'static str {
    match tier {
        Tier::Working => "working",
        Tier::Episodic => "episodic",
        Tier::Provisional => "provisional",
        Tier::Semantic => "semantic",
        Tier::Procedural => "procedural",
        Tier::Relational => "relational",
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

fn split_claim_body(content: &str) -> (String, String) {
    let lines: Vec<&str> = content.lines().collect();
    if lines.is_empty() {
        return (String::new(), String::new());
    }
    let claim = lines[0].to_string();
    let body = if lines.len() > 1 {
        lines[1..].join("\n")
    } else {
        String::new()
    };
    (claim, body)
}
