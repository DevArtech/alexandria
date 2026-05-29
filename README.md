# Alexandria

> A local-first, CLI-first "second brain" designed for how an LLM actually thinks, retrieves, and reasons — not for how a human files paper notes.

Named after the Library of Alexandria, this is a memory substrate built for an LLM consumer. Its prime directive is simple:

> **Maximize useful information per token, and let the agent control retrieval depth.**

Memory lives as plain-text Markdown files (the source of truth) with a rebuildable SQLite index layered on top. Nothing is locked in: delete the index and rebuild it from text at any time.

## Why it exists

Most "AI memory" is just `chunk → embed → top-k cosine`. That discards structure, exact recall, relationships, recency, provenance, and — critically — the ability to say *"I think I know this but can't retrieve it cleanly."* Alexandria keeps semantic search as **one signal among several** inside a structured, typed, provenance-aware, uncertainty-aware system.

Three load-bearing principles:

- **Plain text is the source of truth.** Every index is a rebuildable cache.
- **Honest ignorance is a first-class outcome.** `recall` returns one of five states, not just "rows or nothing."
- **Enforce by structure, not convention.** When a constraint matters (e.g. relational memory never being quoted), it's made impossible to violate.

The full design is in [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

## Status

Alexandria is under active construction. **Milestone 1 (skeleton) is implemented:** the plain-text store, the SQLite + FTS5 index, lexical recall with the five-state return type and response-mode plumbing, and rebuild-from-text. Semantic search, the graph/conflict layer, consolidation, and the provider integrations are planned (see [Roadmap](#roadmap)).

## Build

Requires a recent stable [Rust](https://www.rust-lang.org/tools/install) toolchain.

```bash
git clone <repo-url> alexandria
cd alexandria
cargo build --release
# binary at target/release/alexandria
```

Run the test suite:

```bash
cargo test
```

## Quickstart

```bash
# 1. Initialize a library in the current directory
alexandria init

# 2. Remember something (first line becomes the claim, the rest the body)
alexandria remember "Alexandria uses hybrid fused retrieval, not vector-only"

# Pipe longer content from stdin
cat notes.md | alexandria remember -

# Tag, file into a collection, or set the tier/status
alexandria remember "The user prefers terse answers" --tier relational
alexandria remember "Auth flow uses short-lived JWTs" --collection project-x --tag auth

# 3. Recall (lexical search in M1) with a token budget
alexandria recall "hybrid retrieval"
alexandria recall "auth jwt" --budget 1500 --format json

# 4. Rebuild the index entirely from the Markdown store
alexandria reindex
```

Every command accepts `--format json` for machine/agent consumption and `--library <path>` to target a specific library (otherwise Alexandria discovers the nearest `.alexandria/` from the current directory upward).

### What `recall` returns

Rather than always returning rows, `recall` classifies the result so an agent can adopt an honest posture:

| State | Meaning |
| --- | --- |
| `strong_hit` | High-confidence, discriminating match |
| `weak_hit` | Something matched, but low confidence — hedge |
| `high_confidence_gap` | Relevant memory likely exists but can't be surfaced cleanly *(planned, M2)* |
| `low_confidence_gap` | Topic is adjacent to known domains; nothing precise *(planned, M2)* |
| `nothing` | No meaningful signal |

Each result also carries a recommended **response mode** (`flow` / `humility` / `audit`) so the agent knows whether to use memory invisibly, flag its uncertainty, or expose the full provenance.

## How memory is organized

Memory is typed into tiers, each with its own lifecycle:

- **Working** — ephemeral task scratchpad (not persisted)
- **Episodic** — append-only, timestamped events
- **Provisional** — usable but not yet earned canonical status
- **Semantic** — distilled, curated facts
- **Procedural** — reusable skills and how-tos
- **Relational** — how to work with a specific user (shapes generation only; **never** returned as quotable text)

The atomic unit is an **Engram**: a Markdown file with structured YAML frontmatter (id, tier, status, claim, provenance, confidence, salience, typed links, ...).

### Library layout

```
my-library/
├── .alexandria/
│   ├── config.toml      # providers, budgets, thresholds
│   └── index.db         # SQLite cache (FTS5 + ...) — rebuildable, git-ignored
├── episodic/
├── provisional/
├── semantic/
├── procedural/
├── relational/          # never surfaced as quotable text
├── threads/             # open threads (unresolved_by_design)
├── collections/
└── archive/             # "forgotten" — moved here, never deleted
```

A library is just a directory — `git init` it for free time-travel over your memory.

## Configuration

`.alexandria/config.toml` is created on `init`:

```toml
[providers]
embedder = "none"          # "fastembed" (local), "ollama", "openai" — planned

[budgets]
default_recall_tokens = 2000

[thresholds]
strong_cutoff = -3.0       # BM25 bands for the five-state classifier
weak_cutoff = -8.0
```

## Architecture

Alexandria is a Rust workspace:

- `crates/core` — the library: `store` (plain-text truth), `index` (SQLite/FTS5), `retrieval` (five-state recall), `provider` traits (`Embedder` / `Completer`), `config`, `engram`.
- `crates/cli` — the `alexandria` binary (built on `clap`).

Embeddings and LLM calls sit behind pluggable provider traits with a local-first default, so the system can run fully offline.

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for the complete design, including hybrid retrieval, progressive disclosure, consolidation, the conflict taxonomy, meta-memory, and response modes.

## Roadmap

| Milestone | Scope |
| --- | --- |
| **M1 — Skeleton** ✅ | Plain-text store, SQLite + FTS5 index, `init`/`remember`/`recall` (lexical)/`reindex`, five-state recall + response modes |
| **M2 — Hybrid + budget** | Local embeddings (`fastembed`), semantic search, RRF fusion, density-based gap states, progressive-disclosure context tree, `expand` |
| **M3 — Graph + consolidation** | Typed edges + traversal, conflict taxonomy, provisional promotion ladder, `link`/`trace`/`timeline`, the `reflect`/`consolidate` "sleep" pass |
| **M4 — Relational, shape, meta-memory, modes** | Relational `style` channel, episodic shape index, meta-memory, fast/slow reflection, open-thread surfacing |
| **M5 — Providers & polish** | Ollama + cloud providers, reranker, threshold self-calibration |

## License

MIT
