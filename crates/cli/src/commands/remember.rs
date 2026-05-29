use std::io::{self, Read};
use std::path::PathBuf;

use alexandria_core::{Engram, Index, Library, Status, Tier};
use anyhow::Result;

use crate::OutputFormat;

pub fn run(
    library_path: Option<PathBuf>,
    format: OutputFormat,
    text: String,
    tier: Option<String>,
    status: Option<String>,
    collections: Vec<String>,
    tags: Vec<String>,
) -> Result<()> {
    let content = if text == "-" {
        let mut buf = String::new();
        io::stdin().read_to_string(&mut buf)?;
        buf
    } else {
        text
    };

    let (claim, body) = split_claim_body(&content);

    let tier = match tier.as_deref() {
        Some(s) => Tier::parse(s)?,
        None => Tier::Semantic,
    };

    let status = match status.as_deref() {
        Some(s) => Status::parse(s)?,
        None => Status::Confirmed,
    };

    let library = match library_path {
        Some(p) => Library::discover(Some(&p))?,
        None => Library::discover(None)?,
    };

    let mut engram = Engram::new(claim, body, tier, status);
    engram.collections = collections;
    engram.tags = tags;

    let path = library.write_engram(&engram)?;
    let index = Index::open(&library)?;
    index.upsert(&engram, &path.display().to_string())?;

    match format {
        OutputFormat::Human => {
            println!("Remembered {} ({})", engram.id, engram.claim);
            println!("  tier: {:?}", engram.tier);
            println!("  path: {}", path.display());
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
