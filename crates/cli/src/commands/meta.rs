use std::path::PathBuf;

use alexandria_core::{
    meta_report, rebuild_meta_index, record_correction, record_gap_outcome, Config, Index, Library,
};
use anyhow::{bail, Result};

use crate::OutputFormat;

pub struct MetaOptions {
    pub library_path: Option<PathBuf>,
    pub format: OutputFormat,
    pub domain: Option<String>,
    pub record_correction: bool,
    pub correction_domain: Option<String>,
    pub record_gap: bool,
    pub gap_kind: Option<String>,
    pub gap_confirmed: bool,
}

pub fn run(opts: MetaOptions) -> Result<()> {
    let library = match opts.library_path {
        Some(p) => Library::discover(Some(&p))?,
        None => Library::discover(None)?,
    };
    let config = Config::load(&library.root)?;
    let index = Index::open(&library, &config)?;

    let d = opts
        .domain
        .clone()
        .or(opts.correction_domain.clone())
        .unwrap_or_else(|| "_global".to_string());

    if opts.record_correction {
        record_correction(&library, &d, None)?;
        rebuild_meta_index(&index, &library)?;
    }

    if opts.record_gap {
        let kind = opts.gap_kind.ok_or_else(|| {
            anyhow::anyhow!("--gap-kind is required with --record-gap (e.g. high_confidence_gap)")
        })?;
        if kind != "high_confidence_gap" && kind != "low_confidence_gap" {
            bail!("--gap-kind must be high_confidence_gap or low_confidence_gap");
        }
        record_gap_outcome(&library, &d, &kind, !opts.gap_confirmed)?;
        rebuild_meta_index(&index, &library)?;
    }

    let report = meta_report(&library, &index, opts.domain.as_deref())?;

    match opts.format {
        OutputFormat::Human => {
            if let Some(dom) = &report.domain {
                println!("domain: {dom}");
            }
            println!("reliability: {:.3}", report.reliability);
            println!("recent_corrections (30d): {}", report.recent_corrections);
            println!("gap_false_positive_rate: {:.3}", report.gap_false_positive_rate);
            println!("promotion_reversal_rate: {:.3}", report.promotion_reversal_rate);
            println!(
                "totals: corrections={} gaps={} reversals={}",
                report.total_corrections, report.total_gaps, report.total_reversals
            );
        }
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&report)?),
    }
    Ok(())
}
