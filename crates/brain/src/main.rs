use std::io::BufReader;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use alexandria_core::{
    build_completer, consolidate_fast, consolidate_slow, Config, Index, Library,
};
use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use serde::Serialize;

use alexandria_brain::{
    build_run_prompt, ensure_codex_config, parse_codex_jsonl, resolve_codex_binary,
    resolve_mcp_binary, ParsedCodexRun,
};

#[derive(Debug, Clone, Copy, ValueEnum)]
enum OutputFormat {
    Human,
    Json,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum SandboxMode {
    #[value(name = "read-only")]
    ReadOnly,
    #[value(name = "workspace-write")]
    WorkspaceWrite,
}

impl SandboxMode {
    fn as_flag(self) -> &'static str {
        match self {
            SandboxMode::ReadOnly => "read-only",
            SandboxMode::WorkspaceWrite => "workspace-write",
        }
    }
}

#[derive(Parser)]
#[command(
    name = "alexandria-brain",
    about = "Alexandria second-brain loop powered by Codex",
    version
)]
struct Cli {
    /// Path to library root (defaults to discovering .alexandria/ from cwd)
    #[arg(long, global = true)]
    library: Option<PathBuf>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize library (if needed) and provision Codex + MCP + skill
    Init {
        /// Directory to initialize (defaults to current directory)
        path: Option<PathBuf>,
    },
    /// Run a second-brain task via Codex with Alexandria memory
    Run {
        /// Task prompt for Codex
        task: String,
        #[arg(long)]
        model: Option<String>,
        #[arg(long, value_enum, default_value = "read-only")]
        sandbox: SandboxMode,
        #[arg(long)]
        no_consolidate: bool,
        #[arg(long, value_enum, default_value = "human")]
        format: OutputFormat,
    },
}

#[derive(Serialize)]
struct RunSummary<'a> {
    answer: Option<&'a str>,
    thread_id: Option<&'a str>,
    memory_activity: &'a [alexandria_brain::MemoryActivity],
    usage: Option<&'a alexandria_brain::TokenUsage>,
    consolidated: bool,
    fast_reflection: bool,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Init { path } => cmd_init(path),
        Commands::Run {
            task,
            model,
            sandbox,
            no_consolidate,
            format,
        } => cmd_run(cli.library, task, model, sandbox, no_consolidate, format),
    }
}

fn cmd_init(path: Option<PathBuf>) -> Result<()> {
    let root = path
        .map(|p| p.canonicalize().unwrap_or(p))
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

    let library = if root.join(".alexandria").is_dir() {
        Library::discover(Some(&root))?
    } else {
        Library::init(&root)?
    };

    let mcp_binary = resolve_mcp_binary()?;
    let codex_home = ensure_codex_config(&library.root, &mcp_binary)?;

    println!("Alexandria brain initialized.");
    println!("  library: {}", library.root.display());
    println!("  codex_home: {}", codex_home.display());
    println!("  mcp_server: {}", mcp_binary.display());
    println!("  skill: alexandria-memory");
    Ok(())
}

fn cmd_run(
    library_path: Option<PathBuf>,
    task: String,
    model: Option<String>,
    sandbox: SandboxMode,
    no_consolidate: bool,
    format: OutputFormat,
) -> Result<()> {
    let library = match library_path {
        Some(p) => Library::discover(Some(&p))?,
        None => Library::discover(None)?,
    };

    let mcp_binary = resolve_mcp_binary()?;
    let codex_binary = resolve_codex_binary()?;
    let codex_home = ensure_codex_config(&library.root, &mcp_binary)?;

    let prompt = build_run_prompt(&task);
    let mut cmd = Command::new(&codex_binary);
    cmd.arg("exec")
        .arg("--json")
        .arg("--cd")
        .arg(&library.root)
        .arg("--sandbox")
        .arg(sandbox.as_flag())
        .arg("--skip-git-repo-check")
        .arg(&prompt)
        .env("CODEX_HOME", &codex_home)
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());

    if let Some(model_name) = model {
        cmd.arg("--model").arg(model_name);
    }

    let mut child = cmd.spawn().context("failed to spawn codex exec")?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow::anyhow!("codex exec did not expose stdout"))?;
    let reader = BufReader::new(stdout);
    let parsed = parse_codex_jsonl(reader)?;

    let status = child.wait().context("waiting for codex exec")?;
    if !status.success() && !parsed.failed {
        bail!("codex exec exited with status {status}");
    }

    let mut consolidated = false;
    let mut fast_reflection = false;
    if !no_consolidate {
        post_turn_consolidate(&library, &mut consolidated, &mut fast_reflection)?;
    }

    print_run_result(&parsed, consolidated, fast_reflection, format);
    Ok(())
}

fn post_turn_consolidate(
    library: &Library,
    consolidated: &mut bool,
    fast_reflection: &mut bool,
) -> Result<()> {
    let config = Config::load(&library.root)?;
    let index = Index::open(library, &config)?;
    let completer = build_completer(&config)?;
    consolidate_slow(library, &index, &config, completer.as_deref())?;
    *consolidated = true;
    consolidate_fast(library, &config)?;
    *fast_reflection = true;
    Ok(())
}

fn print_run_result(
    parsed: &ParsedCodexRun,
    consolidated: bool,
    fast_reflection: bool,
    format: OutputFormat,
) {
    match format {
        OutputFormat::Human => {
            if let Some(answer) = &parsed.final_message {
                println!("{answer}");
            }
            if !parsed.memory_activity.is_empty() {
                println!();
                println!("Memory activity:");
                for activity in &parsed.memory_activity {
                    println!(
                        "  {} {} ({})",
                        activity.server, activity.tool, activity.status
                    );
                }
            }
            if let Some(usage) = &parsed.usage {
                println!();
                println!(
                    "Tokens: in={} cached={} out={}",
                    usage.input_tokens, usage.cached_input_tokens, usage.output_tokens
                );
            }
            if consolidated {
                println!("Consolidation: slow + fast reflection complete.");
            } else if fast_reflection {
                println!("Consolidation: fast reflection complete.");
            }
        }
        OutputFormat::Json => {
            let summary = RunSummary {
                answer: parsed.final_message.as_deref(),
                thread_id: parsed.thread_id.as_deref(),
                memory_activity: &parsed.memory_activity,
                usage: parsed.usage.as_ref(),
                consolidated,
                fast_reflection,
            };
            println!("{}", serde_json::to_string_pretty(&summary).unwrap());
        }
    }
}

#[cfg(test)]
mod e2e {
    use super::*;

    #[test]
    #[ignore = "requires authenticated codex CLI"]
    fn codex_run_end_to_end() {
        let dir = tempfile::tempdir().unwrap();
        cmd_init(Some(dir.path().to_path_buf())).unwrap();
        cmd_run(
            Some(dir.path().to_path_buf()),
            "What is Alexandria?".to_string(),
            None,
            SandboxMode::ReadOnly,
            true,
            OutputFormat::Json,
        )
        .unwrap();
    }
}
