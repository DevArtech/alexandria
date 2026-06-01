//! Remote command dispatch: MCP tool calls + shared formatters.

use std::io::{self, Read};

use alexandria_core::{
    ArchiveResult, ConsolidationReport, FastReflectionReport, LinkResult, MetaReport, StyleProfile,
    ThreadsResult, TimelineResult,
};
use anyhow::{bail, Result};
use serde_json::json;

use crate::commands::{
    catalog, consolidate, coverage, expand, map, recall, remember, survey, trace,
};
use crate::OutputFormat;

use super::client::{call_and_parse, call_tool};
use super::config::ResolvedRemote;

fn emit_json(value: &impl serde::Serialize) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

fn emit_typed<T: serde::de::DeserializeOwned + serde::Serialize>(
    remote: &ResolvedRemote,
    tool: &str,
    args: serde_json::Value,
    format: OutputFormat,
    print_human: fn(&T),
) -> Result<()> {
    let value: T = call_and_parse(remote, tool, args)?;
    match format {
        OutputFormat::Human => print_human(&value),
        OutputFormat::Json => emit_json(&value)?,
    }
    Ok(())
}

pub struct RecallRemoteArgs {
    pub query: String,
    pub budget: Option<u32>,
    pub audit: bool,
    pub high_stakes: bool,
    pub collections: Vec<String>,
    pub tags: Vec<String>,
}

pub fn recall(remote: &ResolvedRemote, format: OutputFormat, args: RecallRemoteArgs) -> Result<()> {
    emit_typed(
        remote,
        "recall",
        json!({
            "query": args.query,
            "budget": args.budget,
            "audit": args.audit,
            "high_stakes": args.high_stakes,
            "collections": args.collections,
            "tags": args.tags,
        }),
        format,
        recall::print_human,
    )
}

pub fn remember(
    remote: &ResolvedRemote,
    format: OutputFormat,
    opts: remember::RememberOptions,
) -> Result<()> {
    let content = if opts.text == "-" {
        let mut buf = String::new();
        io::stdin().read_to_string(&mut buf)?;
        buf
    } else {
        opts.text
    };

    let value = call_tool(
        remote,
        "remember",
        json!({
            "text": content,
            "tier": opts.tier,
            "status": opts.status,
            "collections": opts.collections,
            "tags": opts.tags,
            "sources": opts.sources,
            "derived_from": opts.derived_from,
            "surface_when": opts.surface_when,
            "observed": opts.observed,
        }),
    )?;

    match format {
        OutputFormat::Human => remember::print_remote_human(&value)?,
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&value)?),
    }
    Ok(())
}

pub fn catalog(remote: &ResolvedRemote, format: OutputFormat, write_overview: bool) -> Result<()> {
    if write_overview {
        bail!(
            "`catalog --write-overview` only runs against a local library (writes LIBRARY.md on disk)"
        );
    }
    emit_typed(remote, "catalog", json!({}), format, catalog::print_human)
}

pub fn coverage(remote: &ResolvedRemote, format: OutputFormat, topic: String) -> Result<()> {
    emit_typed(
        remote,
        "coverage",
        json!({ "topic": topic }),
        format,
        coverage::print_human,
    )
}

pub fn survey(
    remote: &ResolvedRemote,
    format: OutputFormat,
    topic: String,
    budget: Option<u32>,
    depth: Option<u32>,
) -> Result<()> {
    emit_typed(
        remote,
        "survey",
        json!({ "topic": topic, "budget": budget, "depth": depth }),
        format,
        survey::print_human,
    )
}

pub fn map(
    remote: &ResolvedRemote,
    format: OutputFormat,
    seed: String,
    depth: Option<u32>,
    rel: Vec<String>,
    budget: Option<u32>,
) -> Result<()> {
    emit_typed(
        remote,
        "map",
        json!({ "seed": seed, "depth": depth, "rel": rel, "budget": budget }),
        format,
        map::print_human,
    )
}

pub fn expand(
    remote: &ResolvedRemote,
    format: OutputFormat,
    id: String,
    rel: Option<String>,
) -> Result<()> {
    emit_typed(
        remote,
        "expand",
        json!({ "id": id, "rel": rel }),
        format,
        expand::print_human,
    )
}

pub fn link(
    remote: &ResolvedRemote,
    format: OutputFormat,
    from: String,
    rel: String,
    to: String,
) -> Result<()> {
    let result: LinkResult = call_and_parse(
        remote,
        "link",
        json!({ "from": from, "rel": rel, "to": to }),
    )?;
    match format {
        OutputFormat::Human => link_print_human(&result),
        OutputFormat::Json => emit_json(&result)?,
    }
    Ok(())
}

pub fn trace(remote: &ResolvedRemote, format: OutputFormat, id: String) -> Result<()> {
    emit_typed(
        remote,
        "trace",
        json!({ "id": id }),
        format,
        trace::print_human,
    )
}

pub fn timeline(
    remote: &ResolvedRemote,
    format: OutputFormat,
    since: Option<String>,
    until: Option<String>,
    tier: Option<String>,
) -> Result<()> {
    let result: TimelineResult = call_and_parse(
        remote,
        "timeline",
        json!({ "since": since, "until": until, "tier": tier }),
    )?;
    match format {
        OutputFormat::Human => timeline_print_human(&result),
        OutputFormat::Json => emit_json(&result)?,
    }
    Ok(())
}

pub fn threads(
    remote: &ResolvedRemote,
    format: OutputFormat,
    surface_for: Option<String>,
) -> Result<()> {
    let result: ThreadsResult =
        call_and_parse(remote, "threads", json!({ "surface_for": surface_for }))?;
    match format {
        OutputFormat::Human => threads_print_human(&result),
        OutputFormat::Json => emit_json(&result)?,
    }
    Ok(())
}

pub fn style(remote: &ResolvedRemote, format: OutputFormat, profile: bool) -> Result<()> {
    let result: StyleProfile = call_and_parse(remote, "style", json!({}))?;
    match format {
        OutputFormat::Human => style_print_human(&result, profile),
        OutputFormat::Json => emit_json(&result)?,
    }
    Ok(())
}

pub fn meta(
    remote: &ResolvedRemote,
    format: OutputFormat,
    opts: crate::commands::meta::MetaOptions,
) -> Result<()> {
    let d = opts
        .domain
        .clone()
        .or(opts.correction_domain.clone())
        .unwrap_or_else(|| "_global".to_string());

    if opts.record_gap && opts.gap_kind.is_none() {
        bail!("--gap-kind is required with --record-gap (e.g. high_confidence_gap)");
    }
    if let Some(ref kind) = opts.gap_kind {
        if kind != "high_confidence_gap" && kind != "low_confidence_gap" {
            bail!("--gap-kind must be high_confidence_gap or low_confidence_gap");
        }
    }

    let result: MetaReport = call_and_parse(
        remote,
        "meta",
        json!({
            "domain": opts.domain,
            "record_correction": opts.record_correction,
            "correction_domain": opts.correction_domain,
            "record_gap": opts.record_gap,
            "gap_kind": opts.gap_kind,
            "gap_confirmed": opts.gap_confirmed,
        }),
    )?;
    let _ = d;
    match format {
        OutputFormat::Human => meta_print_human(&result),
        OutputFormat::Json => emit_json(&result)?,
    }
    Ok(())
}

pub fn archive(remote: &ResolvedRemote, format: OutputFormat, id: String) -> Result<()> {
    let result: ArchiveResult = call_and_parse(remote, "archive", json!({ "id": id }))?;
    match format {
        OutputFormat::Human => archive_print_human(&result),
        OutputFormat::Json => emit_json(&result)?,
    }
    Ok(())
}

pub fn consolidate(remote: &ResolvedRemote, format: OutputFormat, dry_run: bool) -> Result<()> {
    if dry_run {
        bail!("`consolidate --dry-run` is not supported against a remote server");
    }
    let result: ConsolidationReport =
        call_and_parse(remote, "consolidate", json!({ "fast": false }))?;
    match format {
        OutputFormat::Human => consolidate::print_human(&result),
        OutputFormat::Json => emit_json(&result)?,
    }
    Ok(())
}

pub fn reflect(remote: &ResolvedRemote, format: OutputFormat, fast: bool) -> Result<()> {
    if fast {
        let result: FastReflectionReport =
            call_and_parse(remote, "consolidate", json!({ "fast": true }))?;
        match format {
            OutputFormat::Human => {
                println!("fast reflection complete");
                println!("briefing: {}", result.briefing_path);
                println!("engrams summarized: {}", result.engrams_summarized);
            }
            OutputFormat::Json => emit_json(&result)?,
        }
    } else {
        let result: ConsolidationReport =
            call_and_parse(remote, "consolidate", json!({ "fast": false }))?;
        match format {
            OutputFormat::Human => {
                println!("slow reflection complete");
                consolidate::print_human(&result);
            }
            OutputFormat::Json => emit_json(&result)?,
        }
    }
    Ok(())
}

fn link_print_human(result: &LinkResult) {
    println!(
        "Linked {} --{}--> {}",
        result.from_id, result.rel, result.to_id
    );
    if result.reciprocal_added {
        println!("  reciprocal edge added");
    }
    if result.target_superseded {
        println!("  target marked superseded");
    }
}

fn archive_print_human(result: &ArchiveResult) {
    println!("Archived {} ({})", result.id, result.claim);
    println!("  path: {}", result.path);
}

fn timeline_print_human(result: &TimelineResult) {
    println!("{} entries", result.count);
    if result.entries.is_empty() {
        println!("(no entries)");
        return;
    }
    for e in &result.entries {
        println!(
            "[{}] {} ({}, {}) @ {}",
            e.id, e.claim, e.tier, e.status, e.created
        );
    }
}

fn threads_print_human(result: &ThreadsResult) {
    if let Some(topic) = &result.surface_for {
        println!("surface_for: {topic}");
    }
    if result.threads.is_empty() {
        println!("(no open threads)");
    }
    for t in &result.threads {
        println!(
            "[{}] {} (last_touched: {}, dormant {:.1}d, triggers: {})",
            t.id,
            t.claim,
            t.last_touched.format("%Y-%m-%d"),
            t.dormant_days,
            t.surface_when.join(", ")
        );
    }
}

fn style_print_human(style: &StyleProfile, profile: bool) {
    if profile {
        println!("verbosity: {:.2}", style.verbosity);
        println!("directness: {:.2}", style.directness);
        println!("hedging: {:.2}", style.hedging);
        println!("pushback_tolerance: {:.2}", style.pushback_tolerance);
        println!("pacing: {}", style.pacing);
        if let Some(ev) = &style.evidence_summary {
            println!(
                "evidence: projects={} task_types={} registers={}",
                ev.projects, ev.task_types, ev.registers
            );
        }
    } else {
        println!(
            "verbosity={:.2} directness={:.2} hedging={:.2} pushback={:.2} pacing={}",
            style.verbosity,
            style.directness,
            style.hedging,
            style.pushback_tolerance,
            style.pacing
        );
    }
}

fn meta_print_human(report: &MetaReport) {
    if let Some(dom) = &report.domain {
        println!("domain: {dom}");
    }
    println!("reliability: {:.3}", report.reliability);
    println!("recent_corrections (30d): {}", report.recent_corrections);
    println!(
        "gap_false_positive_rate: {:.3}",
        report.gap_false_positive_rate
    );
    println!(
        "promotion_reversal_rate: {:.3}",
        report.promotion_reversal_rate
    );
    println!(
        "totals: corrections={} gaps={} reversals={}",
        report.total_corrections, report.total_gaps, report.total_reversals
    );
}
