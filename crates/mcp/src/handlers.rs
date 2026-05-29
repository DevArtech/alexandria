use std::path::PathBuf;

use alexandria_core::{
    build_completer, consolidate_fast, consolidate_slow, list_threads, meta_report,
    rebuild_meta_index, record_correction, record_gap_outcome, style_profile, Config, Engram,
    Graph, Index, Library, Ops, RecallOptions, Rel, Retrieval, Source, Status, Tier,
};
use anyhow::{anyhow, bail, Context, Result};
use serde_json::Value;

use crate::params::{
    ConsolidateParams, ExpandParams, IdParams, LinkParams, MetaParams, RecallParams,
    RememberParams, ThreadsParams, TimelineParams,
};

pub struct ServerState {
    pub library: Library,
    pub config: Config,
    pub index: Index,
}

impl ServerState {
    pub fn open(library_path: Option<PathBuf>) -> Result<Self> {
        let library = match library_path {
            Some(p) => Library::discover(Some(&p))?,
            None => Library::discover(None)?,
        };
        let config = Config::load(&library.root)?;
        let index = Index::open(&library, &config)?;
        Ok(Self {
            library,
            config,
            index,
        })
    }

    pub fn to_json<T: serde::Serialize>(value: &T) -> Result<Value> {
        Ok(serde_json::to_value(value)?)
    }
}

pub fn recall(state: &ServerState, params: RecallParams) -> Result<Value> {
    let retrieval = Retrieval::new(&state.index, &state.config);
    let result = retrieval.recall(
        &params.query,
        params.budget,
        RecallOptions {
            audit: params.audit,
            high_stakes: params.high_stakes,
            domain: params.domain,
        },
    )?;
    ServerState::to_json(&result)
}

pub fn expand(state: &ServerState, params: ExpandParams) -> Result<Value> {
    let retrieval = Retrieval::new(&state.index, &state.config);
    let rel = params
        .rel
        .as_deref()
        .map(Rel::parse)
        .transpose()
        .context("invalid rel")?;
    let result = retrieval.expand(&params.id, rel)?;
    ServerState::to_json(&result)
}

pub fn remember(state: &mut ServerState, params: RememberParams) -> Result<Value> {
    let (claim, body) = split_claim_body(&params.text);

    let tier = match params.tier.as_deref() {
        Some(s) => Tier::parse(s)?,
        None => Tier::Semantic,
    };

    let status = match params.status.as_deref() {
        Some(s) => Status::parse(s)?,
        None => Status::Confirmed,
    };

    let mut engram = Engram::new(claim, body, tier, status);
    engram.collections = params.collections;
    engram.tags = params.tags;

    for s in params.sources {
        engram.source.push(Source::parse_cli(&s)?);
    }
    for id in params.derived_from {
        engram.source.push(Source::derived_from(&id));
    }
    if !params.surface_when.is_empty() {
        engram.surface_when = Some(params.surface_when);
    }

    let path = state.library.write_engram(&engram)?;
    state
        .index
        .upsert(&engram, &path.display().to_string())?;

    ServerState::to_json(&serde_json::json!({
        "id": engram.id,
        "claim": engram.claim,
        "tier": tier_label(engram.tier),
        "status": status_label(engram.status),
        "path": path.display().to_string(),
        "sources": engram.source,
        "surface_when": engram.surface_when,
        "token_cost": Engram::estimate_tokens(&engram.claim),
    }))
}

pub fn link(state: &ServerState, params: LinkParams) -> Result<Value> {
    let rel = Rel::parse(&params.rel)?;
    let ops = Ops::new(&state.library, &state.index);
    let result = ops.link(&params.from, rel, &params.to)?;
    ServerState::to_json(&result)
}

pub fn trace(state: &ServerState, params: IdParams) -> Result<Value> {
    let graph = Graph::new(&state.index);
    let result = graph.trace(&params.id)?;
    ServerState::to_json(&result)
}

pub fn timeline(state: &ServerState, params: TimelineParams) -> Result<Value> {
    let graph = Graph::new(&state.index);
    let tier = params
        .tier
        .as_deref()
        .map(Tier::parse)
        .transpose()?;
    let result = graph.timeline(
        params.since.as_deref(),
        params.until.as_deref(),
        tier,
    )?;
    ServerState::to_json(&result)
}

pub fn threads(state: &ServerState, params: ThreadsParams) -> Result<Value> {
    let result = list_threads(
        &state.library,
        &state.index,
        params.surface_for.as_deref(),
    )?;
    ServerState::to_json(&result)
}

pub fn style(state: &ServerState) -> Result<Value> {
    let profile = style_profile(&state.library, None)?;
    ServerState::to_json(&profile)
}

pub fn meta(state: &mut ServerState, params: MetaParams) -> Result<Value> {
    let d = params
        .domain
        .clone()
        .or(params.correction_domain.clone())
        .unwrap_or_else(|| "_global".to_string());

    if params.record_correction {
        record_correction(&state.library, &d, None)?;
        rebuild_meta_index(&state.index, &state.library)?;
    }

    if params.record_gap {
        let kind = params.gap_kind.ok_or_else(|| {
            anyhow!("gap_kind is required when record_gap is true (high_confidence_gap or low_confidence_gap)")
        })?;
        if kind != "high_confidence_gap" && kind != "low_confidence_gap" {
            bail!("gap_kind must be high_confidence_gap or low_confidence_gap");
        }
        record_gap_outcome(&state.library, &d, &kind, !params.gap_confirmed)?;
        rebuild_meta_index(&state.index, &state.library)?;
    }

    let report = meta_report(&state.library, &state.index, params.domain.as_deref())?;
    ServerState::to_json(&report)
}

pub fn archive(state: &ServerState, params: IdParams) -> Result<Value> {
    let ops = Ops::new(&state.library, &state.index);
    let result = ops.archive(&params.id)?;
    ServerState::to_json(&result)
}

pub fn consolidate(state: &ServerState, params: ConsolidateParams) -> Result<Value> {
    if params.fast {
        let report = consolidate_fast(&state.library, &state.config)?;
        return ServerState::to_json(&report);
    }

    let completer = build_completer(&state.config)?;
    let report = consolidate_slow(
        &state.library,
        &state.index,
        &state.config,
        completer.as_deref(),
    )?;
    ServerState::to_json(&report)
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
