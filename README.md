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

Alexandria is under active construction. **Milestone 4 (relational, shape, meta-memory, modes) is implemented**, on top of M1 (plain-text store, FTS5 index, five-state recall), M2 (hybrid `sqlite-vec` semantic search, RRF fusion, density-based gap states, context trees, `expand`), and M3 (typed-edge graph + traversal, conflict taxonomy, provenance + `trace`, the promotion ladder, the `consolidate`/`reflect` "sleep" pass). M4 adds:

- **Relational `style` channel** — `style --profile` assembles structured generation parameters (verbosity, directness, hedging, pushback tolerance, pacing) from relational Engrams, which are **never returned as quotable text**.
- **Meta-memory** — an append-only `meta_log/` (survives `reindex`) tracking per-domain reliability, corrections, gap false-positive rates, and promotion-reversals; surfaced via `meta` and fed into the posture judge.
- **Response modes** — `recall` recommends `flow` / `humility` / `audit`, escalating to humility on weak/gap states, provisional content, conflict edges, or weak domain reliability, and to audit on `--audit` / `--high-stakes`.
- **Episodic shape index** — a sixth retrieval signal that matches by problem-arc similarity, not just topic.
- **Open-thread surfacing** — `remember --surface-when topic:…` plus `threads --surface-for <topic>`.
- **Fast/slow reflection** — `reflect --fast` writes non-canonical briefing material to `.alexandria/fast_reflections/`; the slow pass remains the only path to canonical memory.

Full provider integrations (Ollama, cloud) and a reranker are planned (see [Roadmap](#roadmap)).

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

# Record provenance: where a claim came from, or what it was derived from
alexandria remember "User said use Rust" --tier episodic --source conversation:conv_1
alexandria remember "Alexandria is written in Rust" --derived-from eng_89187aa4

# 3. Recall (hybrid lexical + semantic, RRF fusion) with a token budget
alexandria recall "hybrid retrieval"
alexandria recall "auth jwt" --budget 1500 --format json

# 4. Expand a hit to full body and linked claims
alexandria expand eng_7f3a2c
alexandria expand eng_7f3a2c --rel depends_on --format json

# 5. Relate engrams with typed edges (reciprocals added automatically)
alexandria link eng_aaa supports eng_bbb
alexandria link eng_new supersedes eng_old        # old is marked superseded + archived
alexandria link eng_aaa conflicts_confirmed eng_ccc

# 6. Walk provenance, view the timeline
alexandria trace eng_7f3a2c
alexandria timeline --since 2026-05-01 --tier episodic

# 7. Archive (never deleted) and run the consolidation "sleep" pass
alexandria archive eng_old      # alias: alexandria forget eng_old
alexandria consolidate          # dedupe, promote/demote, decay, re-summarize
alexandria reflect --fast       # quick, non-canonical briefing for the next session

# 8. Open threads, relational style, and meta-memory
alexandria remember "Postgres vs SQLite for the cache?" \
  --status unresolved_by_design --surface-when topic:database
alexandria threads --surface-for database
alexandria style --profile                      # generation params, never quotable bodies
alexandria meta db                              # reliability / corrections / gap rates for a domain
alexandria meta --record-correction --correction-domain db
alexandria recall "cache strategy" --audit      # force full-provenance posture

# 9. Rebuild the index entirely from the Markdown store
alexandria reindex
```

Every command accepts `--format json` for machine/agent consumption and `--library <path>` to target a specific library (otherwise Alexandria discovers the nearest `.alexandria/` from the current directory upward).

### What `recall` returns

Rather than always returning rows, `recall` classifies the result so an agent can adopt an honest posture:

| State | Meaning |
| --- | --- |
| `strong_hit` | High-confidence, discriminating match |
| `weak_hit` | Something matched, but low confidence — hedge |
| `high_confidence_gap` | Relevant memory likely exists but can't be surfaced cleanly |
| `low_confidence_gap` | Topic is adjacent to known domains; nothing precise |
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
│   ├── config.toml         # providers, budgets, thresholds
│   ├── index.db            # SQLite cache (FTS5 + sqlite-vec + ...) — rebuildable, git-ignored
│   ├── meta_log/           # append-only meta-memory events — survives reindex
│   └── fast_reflections/   # non-canonical fast-pass briefings (never scanned as memory)
├── episodic/
├── provisional/
├── semantic/
├── procedural/
├── relational/          # never surfaced as quotable text
├── threads/             # open threads (unresolved_by_design)
├── collections/         # roll-up summaries written by `consolidate`
└── archive/             # "forgotten" / superseded — moved here, never deleted
```

A library is just a directory — `git init` it for free time-travel over your memory.

## Configuration

`.alexandria/config.toml` is created on `init`:

```toml
[providers]
embedder = "fastembed"     # "fastembed" (local), "hash" (offline/tests), "none" (disabled)

[providers.embedding]
# model = "BGESmallENV15"  # fastembed model id

[budgets]
default_recall_tokens = 2000

[thresholds]
rrf_k = 60
strong_cutoff = 0.03                  # fused RRF score bands among distance-qualified hits
weak_cutoff = 0.015
min_corroborating_signals = 2         # signals (lexical + semantic) required for strong_hit
semantic_weak_max_distance = 0.55     # max L2 distance to count as a (weak) semantic match
semantic_strong_max_distance = 0.38   # tighter distance required to reach strong_hit
density_radius = 0.8                  # neighborhood shell for high_confidence_gap
density_min_count = 3                 # min neighbors in that shell to call it "dense"
centroid_radius = 0.72                # near-a-collection band for low_confidence_gap
```

The distance thresholds are L2 distances in embedding space and **must be tuned per embedder** — the values above are oriented to `fastembed`; the `hash` embedder's distances are much larger, so its tests scale these up (roughly `weak ≈ 1.25`, `centroid ≈ 1.4`, `density ≈ 1.55`). For the gap states to be reachable, the radii must keep the ordering **relevance shell < centroid band < density shell** (`semantic_weak_max_distance < centroid_radius < density_radius`); otherwise a query can never be "far from any clean hit yet inside a dense neighborhood."

## Architecture

Alexandria is a Rust workspace:

- `crates/core` — the library: `store` (plain-text truth), `index` (SQLite/FTS5 + sqlite-vec), `retrieval` (hybrid RRF + five-state recall + context tree + posture judge), `graph` (traversal/`trace`/`timeline`), `consolidate` (slow + fast passes), `meta` (meta-memory), `shape`, `style`, `threads`, `ops`, `provider` (`Embedder` / `Completer`), `config`, `engram`.
- `crates/cli` — the `alexandria` binary (built on `clap`).

Embeddings and LLM calls sit behind pluggable provider traits with a local-first default. The default `fastembed` provider downloads an ONNX model on first use (~130MB); set `embedder = "hash"` in config for fully offline operation (no semantic quality guarantees). `expand` does not load the embedder.

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for the complete design, including hybrid retrieval, progressive disclosure, consolidation, the conflict taxonomy, meta-memory, and response modes.

## Roadmap

| Milestone | Scope |
| --- | --- |
| **M1 — Skeleton** ✅ | Plain-text store, SQLite + FTS5 index, `init`/`remember`/`recall` (lexical)/`reindex`, five-state recall + response modes |
| **M2 — Hybrid + budget** ✅ | Local embeddings (`fastembed` + `hash` for tests), semantic search, RRF fusion, density-based gap states, progressive-disclosure context tree, `expand` |
| **M3 — Graph + consolidation** ✅ | Typed edges + traversal, conflict taxonomy, provenance (`--source`/`--derived-from` + `trace`), provisional promotion ladder, `link`/`timeline`/`archive`, the `reflect`/`consolidate` "sleep" pass |
| **M4 — Relational, shape, meta-memory, modes** ✅ | Relational `style` channel, episodic shape index, meta-memory (`meta`), response modes (`--audit`/`--high-stakes`), fast/slow reflection (`reflect --fast`), open-thread surfacing (`--surface-when` / `threads --surface-for`) |
| **M5 — Providers & polish** | Ollama + cloud providers, reranker, threshold self-calibration |

## License

MIT
