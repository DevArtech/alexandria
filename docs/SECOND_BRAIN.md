# Alexandria Second-Brain Loop (Codex)

Alexandria's memory engine works standalone via the `alexandria` CLI. The **second-brain loop** is an optional packaged layer on top: it drives [OpenAI Codex](https://developers.openai.com/codex) as the agent, wires Alexandria in as an MCP tool server, and runs consolidation after each turn.

```
Task prompt
    → alexandria-brain run
        → codex exec --json (CODEX_HOME = <library>/.alexandria/codex/)
            ↔ alexandria-mcp (stdio MCP tools)
                → alexandria-core (store + index)
        → consolidate_slow + consolidate_fast (post-turn)
```

Nothing in `crates/core` or `crates/cli` changes behavior — they remain the standalone memory product.

---

## Prerequisites

1. **Rust toolchain** — build Alexandria from source (see [README](../README.md)).
2. **OpenAI Codex CLI** — install and authenticate separately:
   - Install the `codex` binary per [OpenAI's Codex docs](https://developers.openai.com/codex).
   - Log in via your existing Codex login flow, or set `CODEX_API_KEY`.
   - Alexandria brain does **not** manage Codex credentials; it detects and reports if `codex` is missing.

3. **Put `alexandria-mcp` on PATH** — Codex spawns it as a subprocess. Either:
   - Add `target/release/` to your PATH after `cargo build --release`, or
   - Set `ALEXANDRIA_MCP=/abs/path/to/alexandria-mcp` when running brain.

---

## Build

```bash
cargo build --release
# produces:
#   target/release/alexandria        (standalone memory CLI)
#   target/release/alexandria-mcp    (MCP server for Codex)
#   target/release/alexandria-brain  (second-brain orchestrator)
```

Recommended: symlink or copy the three binaries into a directory on your PATH.

---

## Quickstart

```bash
# 1. Initialize a library + provision Codex config + install skill
alexandria-brain init
# or: alexandria-brain init /path/to/my-library

# 2. Run a task (Codex agent + Alexandria memory)
alexandria-brain run "Summarize what we decided about the auth architecture"

# JSON output (answer + memory activity + token usage)
alexandria-brain run "What open threads exist about pricing?" --format json

# Skip post-turn consolidation (faster, for debugging)
alexandria-brain run "Quick lookup" --no-consolidate

# Codex sandbox modes
alexandria-brain run "Refactor the README" --sandbox workspace-write
alexandria-brain run "Explain hybrid retrieval" --sandbox read-only
```

---

## What `init` provisions

For library `<lib>/`, brain creates an isolated Codex home at `<lib>/.alexandria/codex/`:

```
<lib>/.alexandria/codex/
├── config.toml                          # MCP server registration
└── skills/alexandria-memory/SKILL.md    # Codex skill (recall→act→remember loop)
```

**`config.toml`** registers the MCP server:

```toml
[mcp_servers.alexandria]
command = "/abs/path/to/alexandria-mcp"
args = ["--library", "/abs/path/to/lib"]
enabled = true
```

**`alexandria-memory` skill** teaches Codex to:
- Call `recall` before answering
- Honor five-state results and `response_mode` (`flow` / `humility` / `audit`)
- `remember` durable facts after acting
- Consult `style` for tone (never quote relational memory)
- Use `expand` only when worth the tokens

On `run`, brain sets `CODEX_HOME=<lib>/.alexandria/codex/` and sends a minimal prompt:

```
Use the $alexandria-memory skill.

<your task>
```

The explicit `$alexandria-memory` mention loads the skill deterministically.

---

## What `run` does

1. Resolves the library (discovers nearest `.alexandria/` or uses `--library`).
2. Lazy-provisions Codex config if missing (`ensure_codex_config`).
3. Preflights: `codex` and `alexandria-mcp` must be on PATH (or `ALEXANDRIA_MCP` set).
4. Spawns:
   ```bash
   codex exec --json --cd <lib> --sandbox <mode> --skip-git-repo-check "<prompt>"
   ```
   with `CODEX_HOME` pointed at the provisioned directory.
5. Parses JSONL stdout:
   - Final answer: `item.completed` where `item.type == "agent_message"`
   - Memory breadcrumbs: `mcp_tool_call` items from server `alexandria`
   - Token usage: `turn.completed.usage`
   - Failures: `turn.failed` / fatal `error`
6. Unless `--no-consolidate`, runs `consolidate_slow` then `consolidate_fast` via core.
7. Prints the final answer plus a short memory-activity / token summary.

---

## MCP tools exposed to Codex

| Tool | Purpose |
| --- | --- |
| `recall` | Hybrid fused retrieval; five-state + response_mode |
| `expand` | Full body + links (relational suppressed) |
| `remember` | Write a new engram |
| `link` | Typed edge between engrams |
| `trace` | Provenance walk |
| `timeline` | Episodic view over time |
| `threads` | Open threads (`unresolved_by_design`) |
| `style` | Relational generation params (never quotable) |
| `meta` | Meta-memory report / record correction or gap |
| `archive` | Move engram to archive |
| `consolidate` | Slow consolidation or fast reflection (`fast: true`) |

All tools return JSON matching the core `Serialize` result types (same shapes as `alexandria --format json`).

---

## Sandbox and memory persistence

Codex's file sandbox (`--sandbox read-only` or `workspace-write`) controls what **Codex itself** can write to disk. Alexandria memory is written through the **MCP server process** (`alexandria-mcp`), which operates outside Codex's sandbox.

This means you can run:

```bash
alexandria-brain run "Research and remember findings" --sandbox read-only
```

Codex cannot modify your repo files, but it **can still persist memory** via MCP `remember` / `link` tools. Use `read-only` when you want the agent to consult and update memory without touching workspace files.

For tasks that need file edits, use `--sandbox workspace-write`.

---

## Standalone vs loop

| Mode | When to use |
| --- | --- |
| **`alexandria` CLI** | Direct memory ops, scripting, manual curation, testing retrieval |
| **`alexandria-mcp`** | Wire Alexandria into any MCP client (Codex, Cursor, etc.) |
| **`alexandria-brain`** | Packaged "second brain" turn: Codex agent + memory + post-turn consolidation |

The core memory model, retrieval, consolidation, and CLI are unchanged. Brain is a thin orchestration layer.

---

## Troubleshooting

| Problem | Fix |
| --- | --- |
| `codex not found on PATH` | Install Codex CLI and ensure it is on PATH |
| `alexandria-mcp not found on PATH` | Build release binaries and add to PATH, or set `ALEXANDRIA_MCP` |
| Codex auth errors | Run Codex login separately; brain does not manage keys |
| Memory not persisting | Check MCP server is registered in `<lib>/.alexandria/codex/config.toml`; re-run `alexandria-brain init` |
| Skill not loading | Prompt must include `$alexandria-memory`; verify skill file exists under `codex/skills/` |

---

## See also

- [README](../README.md) — build, quickstart, configuration
- [OVERVIEW.md](OVERVIEW.md) — full application summary
- [ARCHITECTURE.md](ARCHITECTURE.md) — memory model and retrieval design
