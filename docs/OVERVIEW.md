# Alexandria — Overview & How to Use

A local-first, CLI-first memory substrate (a "second brain") built for an LLM consumer. Complete across all five planned milestones: ~6,700 lines of Rust core, a 16-verb CLI, 63 tests passing, zero clippy warnings.

For the full design rationale see [ARCHITECTURE.md](ARCHITECTURE.md). For a shorter pitch see the [README](../README.md).

---

## Part 1 — What it is

Alexandria stores memory as **plain-text Markdown + YAML frontmatter** (the source of truth) with a **single rebuildable SQLite index** layered on top. Its guiding contract is *maximize useful information per token, and let the agent control retrieval depth*, and its most distinctive trait is **honest uncertainty** — retrieval can say "I think I know this but can't surface it cleanly" instead of always returning rows.

Three load-bearing principles:

- **Plain text is the source of truth.** Every index is a rebuildable cache (`reindex` rebuilds it from text).
- **Honest ignorance is a first-class outcome.** `recall` returns one of five states, not just "rows or nothing."
- **Enforce by structure, not convention.** When a constraint matters (relational memory never being quoted; fast-pass briefings never being canonical), it is made physically impossible to violate.

---

## Part 2 — Architecture

Alexandria is a Rust workspace with four crates:

- **`crates/core`** — the library:
  - `store` — Markdown + frontmatter canonical store
  - `index` — SQLite: FTS5 (lexical) + sqlite-vec (semantic + shape vectors) + edges + meta tables
  - `retrieval` — hybrid RRF fusion, five-state recall, budget-aware context tree, posture judge
  - `graph` — recursive-CTE traversal, `trace` (provenance), `timeline`
  - `consolidate` — slow "sleep" pass + fast briefing pass
  - `meta` — append-only reliability log + derived tables
  - `shape`, `style`, `threads`, `ops` — supporting features
  - `provider` — `Embedder` / `Completer` / `Reranker` traits (fastembed, hash, Ollama, OpenAI, Anthropic)
  - `config`, `engram`, `error`
- **`crates/cli`** — the `alexandria` binary (built on `clap`), 16 verbs, human + `--format json` output.
- **`crates/mcp`** — `alexandria-mcp`: rmcp stdio MCP server exposing memory verbs as tools for Codex and other MCP clients.
- **`crates/brain`** — `alexandria-brain`: packaged second-brain loop that drives Codex via `codex exec --json`, parses the run, and consolidates post-turn.

Layering: `Store → Index → Retrieval → Consolidation → CLI`, with Providers as a cross-cutting synchronous trait boundary and Meta-memory feeding both retrieval calibration and the posture judge. The Codex loop adds `brain → codex ↔ mcp → core` on top without changing core behavior.

### Memory model

- **Six tiers**: `working` (ephemeral), `episodic` (append-only), `provisional` (usable but unearned), `semantic` (curated), `procedural` (how-tos), and `relational` (interaction style — structurally barred from ever being quoted).
- **Atomic unit — the Engram**: a Markdown file with rich frontmatter (id, tier, status, claim, `source` provenance, confidence, salience, typed `links`, `surface_when`, `shape_ref`). IDs are 64-bit content-addressed with collision detection on write.
- **Promotion ladder**: `episodic → provisional → semantic`, with conflict-driven demotion.
- **Status lifecycle**: `confirmed`, `provisional`, `unresolved_by_design` (open threads, never collapsed), `superseded`, `archived` (never hard-deleted).

### Retrieval

Six fused signals via Reciprocal Rank Fusion: **lexical** (FTS5/BM25), **semantic** (sqlite-vec KNN), **shape** (problem-arc similarity), graph, temporal, structural. On top:

- **Five-state classification** gated on real semantic distance + neighborhood density: `strong_hit`, `weak_hit`, `high_confidence_gap`, `low_confidence_gap`, `nothing`.
- **Progressive disclosure**: a token-budgeted context tree of summaries first; `expand` drills into bodies + typed links on demand.
- **Response modes**: a rule-based posture judge emits `flow` / `humility` / `audit` from recall state, provisional content, conflict edges, meta-reliability, and `--audit` / `--high-stakes`.
- **Optional reranker**: local fastembed cross-encoder, applied after state classification.

### Graph, provenance & consolidation

- **Typed edges** with the full conflict taxonomy (`conflicts_confirmed`, `tension_possible`, `context_qualified`, `coexists`, `supersedes`/`superseded_by`) and multi-hop recursive-CTE traversal.
- **Provenance**: `--source` / `--derived-from`; `trace` walks the DAG to first-party sources and reports effective confidence (derived-premise bound × conflict penalty, ignoring resolved/superseded conflicts).
- **Consolidation "sleep" pass**: dedupe + merge, promote/demote, salience decay (30-day half-life), shape extraction, collection roll-ups. Two-track: the **fast** pass writes non-canonical briefings to `.alexandria/fast_reflections/`; the **slow** pass is the only path to canonical memory.
- **Meta-memory**: append-only `meta_log/*.jsonl` (survives `reindex`) → per-domain reliability that drives bounded score-calibration and humility escalation.

### Providers

Synchronous `Embedder` / `Completer` / `Reranker` traits, local-first by default: **fastembed** (default local ONNX), **hash** (offline/deterministic for tests), **Ollama**, **OpenAI**, **Anthropic**. Embedding dimensions are probed once and cached in `index_meta` so HTTP providers skip the (billed) probe on re-open; vec tables are created at the embedder's real dimension.

---

## Part 3 — How to Use

### Install / build

Requires a recent stable [Rust](https://www.rust-lang.org/tools/install) toolchain.

```bash
git clone <repo-url> alexandria
cd alexandria
cargo build --release          # alexandria, alexandria-mcp, alexandria-brain
cargo test                     # run the suite
```

Put the binary on your `PATH` (or use `cargo run -- <args>` during development):

```bash
cp target/release/alexandria ~/.local/bin/    # or wherever your PATH points
cp target/release/alexandria-mcp ~/.local/bin/
cp target/release/alexandria-brain ~/.local/bin/
```

### Second-brain loop (Codex)

The memory engine works standalone. Optionally, use the packaged Codex loop:

```bash
# Requires Codex CLI installed + authenticated (see SECOND_BRAIN.md)
alexandria-brain init
alexandria-brain run "Summarize project decisions about auth"
alexandria-brain run "Remember this finding" --sandbox read-only
```

Brain provisions an isolated `CODEX_HOME` at `<library>/.alexandria/codex/` with MCP config and the bundled `alexandria-memory` skill. Codex calls `recall` before acting and `remember` after; brain runs consolidation when the turn completes.

See [SECOND_BRAIN.md](SECOND_BRAIN.md) for prerequisites, sandbox behavior (read-only still persists memory via MCP), and troubleshooting.

### Global flags

Every command accepts:

- `--library <path>` — target a specific library (otherwise Alexandria discovers the nearest `.alexandria/` from the current directory upward).
- `--format human|json` — `human` (default) for reading; `json` for agent/script consumption.

### 1. Create a library

A library is just a directory. `git init` it for free time-travel over your memory.

```bash
mkdir my-brain && cd my-brain
alexandria init
git init                       # optional but recommended
```

This creates `.alexandria/config.toml`, the tier directories (`semantic/`, `episodic/`, …), and the index.

### 2. Write memories (`remember`)

The first line of the text becomes the **claim**; the rest is the body.

```bash
# Basic
alexandria remember "Alexandria uses hybrid fused retrieval, not vector-only"

# From stdin (for longer content)
cat notes.md | alexandria remember -

# Choose tier/status, tag, and file into collections
alexandria remember "The user prefers terse, direct answers" --tier relational
alexandria remember "Auth uses short-lived JWTs" --collection project-x --tag auth

# Record provenance (told vs. derived)
alexandria remember "User said: use Rust" --tier episodic --source conversation:conv_1
alexandria remember "Alexandria is written in Rust" --derived-from eng_89187aa4

# Open thread that should resurface on a topic
alexandria remember "Postgres vs SQLite for the cache?" \
  --status unresolved_by_design --surface-when topic:database
```

### 3. Retrieve (`recall` + `expand`)

```bash
# Hybrid recall with a token budget; returns a five-state outcome + recommended posture
alexandria recall "how does auth work" --budget 1500

# JSON for agents (includes state, response_mode, token costs, signals)
alexandria recall "auth jwt" --format json

# Force the most cautious posture for high-stakes answers
alexandria recall "billing logic" --audit
alexandria recall "production migration plan" --high-stakes

# Drill from a summary into the full body + linked claims
alexandria expand eng_7f3a2c
alexandria expand eng_7f3a2c --rel depends_on --format json
```

**Reading the result:** the `state` field tells you how to act —

| State | What it means for you |
| --- | --- |
| `strong_hit` | High-confidence, discriminating match — use it (mode `flow`). |
| `weak_hit` | Something matched but low confidence — hedge (mode `humility`). |
| `high_confidence_gap` | Relevant memory likely exists but can't be surfaced cleanly — say so. |
| `low_confidence_gap` | Adjacent to known domains but nothing precise. |
| `nothing` | No basis to answer from memory. |

### 4. Relate, trace, and review

```bash
# Typed edges (reciprocals added automatically; supersede archives the old one)
alexandria link eng_aaa supports eng_bbb
alexandria link eng_new supersedes eng_old
alexandria link eng_aaa conflicts_confirmed eng_ccc

# Walk provenance back to first-party sources + effective confidence
alexandria trace eng_7f3a2c

# Episodic view over time
alexandria timeline --since 2026-05-01 --tier episodic

# Surface open threads relevant to a topic
alexandria threads --surface-for database
```

### 5. Maintain memory (`consolidate` / `reflect`)

```bash
# Slow "sleep" pass: dedupe, promote/demote, decay, re-summarize (the only path to canonical memory)
alexandria consolidate

# Fast pass: quick, non-canonical briefing for the next session (written to .alexandria/fast_reflections/)
alexandria reflect --fast

# Archive (never deleted)
alexandria archive eng_old      # alias: alexandria forget eng_old

# Rebuild the index entirely from the Markdown store
alexandria reindex
```

### 6. Relational style & meta-memory

```bash
# Generation parameters distilled from relational engrams (NEVER quotable bodies)
alexandria style --profile

# Inspect / feed meta-memory (reliability drives posture + score calibration)
alexandria meta db
alexandria meta --record-correction --correction-domain db
alexandria meta --record-gap --gap-kind high_confidence_gap --correction-domain db
```

### Typical agent loop

1. `recall "<intent>" --budget N --format json` → inspect `state` + `response_mode`.
2. If `flow`/`strong_hit`, `expand` the few engrams worth the tokens and use them.
3. If a gap or `humility`, tell the user you're unsure rather than fabricating.
4. After the session, `remember` new facts (with `--source`/`--derived-from`) and run `consolidate` (or `reflect --fast` for immediate continuity).

---

## Part 4 — Configuration

`.alexandria/config.toml` is created by `init`. Key sections:

```toml
[providers]
embedder = "fastembed"      # "fastembed" (local), "ollama", "openai", "hash" (offline/tests)
# completer = "ollama"      # "ollama", "openai", "anthropic" — used by consolidation/shape

[providers.ollama]
# base_url = "http://localhost:11434"
# embed_model = "nomic-embed-text"
# complete_model = "llama3"

[providers.openai]
# embed_model = "text-embedding-3-small"
# complete_model = "gpt-4o-mini"
# api_key_env = "OPENAI_API_KEY"

[providers.anthropic]
# complete_model = "claude-3-5-haiku-20241022"
# api_key_env = "ANTHROPIC_API_KEY"

[reranker]
# enabled = false           # set true to activate the local fastembed cross-encoder
# model = "JINARerankerV1TurboEn"

[calibration]
# enabled = true
# score_weight_floor = 0.5

[budgets]
default_recall_tokens = 2000

[thresholds]
rrf_k = 60
strong_cutoff = 0.03
weak_cutoff = 0.015
min_corroborating_signals = 2
semantic_weak_max_distance = 0.55      # max L2 distance for a (weak) semantic match
semantic_strong_max_distance = 0.38    # tighter distance for strong_hit
density_radius = 0.8                   # neighborhood shell for high_confidence_gap
density_min_count = 3
centroid_radius = 0.72                 # near-a-collection band for low_confidence_gap
```

**Tuning notes:**

- Distance thresholds are L2 distances in embedding space and **must be tuned per embedder**. The defaults are oriented to `fastembed`; the `hash` embedder's distances are much larger.
- For the gap states to be reachable, keep the ordering **`semantic_weak_max_distance < centroid_radius < density_radius`**.
- The `[providers]` default `fastembed` downloads an ONNX model (~130 MB) on first use; set `embedder = "hash"` for fully offline operation (no semantic quality guarantees).

### On-disk layout

```
my-brain/
├── .alexandria/
│   ├── config.toml         # providers, budgets, thresholds
│   ├── index.db            # SQLite cache — rebuildable, git-ignored
│   ├── meta_log/           # append-only meta-memory events — survives reindex
│   ├── fast_reflections/   # non-canonical fast-pass briefings (never scanned as memory)
│   └── codex/              # isolated CODEX_HOME (MCP config + skills) when using brain
├── episodic/   provisional/   semantic/   procedural/
├── relational/             # never surfaced as quotable text
├── threads/                # open threads (unresolved_by_design)
├── collections/            # roll-up summaries written by `consolidate`
└── archive/                # "forgotten" / superseded — moved here, never deleted
```

---

## Part 5 — Status & known deferrals

**Complete:** all five milestones (M1 skeleton, M2 hybrid retrieval, M3 graph + consolidation, M4 relational/shape/meta-memory/modes, M5 providers + reranker + calibration) plus the Codex second-brain loop (`alexandria-mcp`, `alexandria-brain`, `alexandria-memory` skill). 70+ tests passing, clippy-clean.

**Deliberate deferrals (not bugs):**

- Meta-memory signals are operator-driven (`meta --record-correction` / `--record-gap`) rather than auto-detected from conversation.
- Fast-pass briefings and collection roll-ups are write-only artifacts (meant for the agent to load between sessions).
- Self-calibration is bounded score down-weighting in low-reliability domains, not full per-domain threshold self-tuning.
