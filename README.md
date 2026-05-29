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

**All five milestones are complete.** Alexandria is fully implemented through M5.

M1–M4 delivered the core memory engine: plain-text store, hybrid five-signal retrieval (lexical + semantic + shape + graph + temporal) with five-state recall and budget-aware context trees, typed-edge graph with conflict taxonomy, provenance tracing, the consolidation "sleep" pass, the relational `style` channel, meta-memory, response modes, open-thread surfacing, and fast/slow reflection.

**M5 adds:**

- **Pluggable cloud + local providers** — Ollama (`embedder = "ollama"`, `completer = "ollama"`), OpenAI (`embedder = "openai"`, `completer = "openai"`), Anthropic (`completer = "anthropic"`), alongside the existing local `fastembed` default. HTTP providers probe their embedding dimension once and cache it in `index_meta`, skipping the network call on subsequent opens.
- **Cross-encoder reranker** — local fastembed reranker (`[reranker] enabled = true`), applied after RRF fusion. Five-state classification always runs on fused RRF scores before reranking, so the state bands are independent of reranker ordering.
- **Bounded meta-driven self-calibration** — fused scores in domains with low reliability (≥ 5 corrections floors at `0.5`, below the `0.6` posture threshold) are down-weighted and the posture judge switches from `flow` to `humility`, even when the immediate retrieval looks confident.
- **Provider traits are synchronous** — `Embedder::embed` and `Completer::complete` are plain `fn`, not `async fn`. All HTTP implementations already used `reqwest::blocking`; removing the async decoration eliminates the latent nested-runtime panic if either trait is ever called from a real Tokio context.

**Post-M5 polish — memory self-awareness:**

Alexandria now surfaces signals it already computed but previously hid — so an agent can reason about *how much* it knows, *where depth is hiding*, and *when a topic wants expansion instead of a top-k answer*:

- **`coverage`** — memory-density x-ray for a topic (canonical/provisional counts, open threads, provenance depth, recency, detail ratio, meta reliability, recommended next step).
- **`survey`** — exhaustive-but-budgeted topic traversal: enumerate the relevant neighborhood cheaply, with per-hit `body_tokens` and `expansion_value` ("1 terse claim, 700 words behind it").
- **`map`** — concept graph from an engram id or topic via typed-edge traversal, budget-trimmed and grouped by rel type (includes mermaid in human output).
- **Facet-aware recall** — auto-detects matching collections/tags from the query (`recall.auto_facet = true` by default), blends them as an additive `facet` signal, and suggests explicit `--collection`/`--tag` scoping. Heuristic facet matches do **not** force `strong_hit` — only explicit filters do.
- **Source freshness** — optional `observed` timestamp on provenance sources; `recall`/`survey`/`expand`/`trace` surface staleness warnings when sources age past `freshness.stale_after_days`.

## Build

Requires a recent stable [Rust](https://www.rust-lang.org/tools/install) toolchain.

```bash
git clone <repo-url> alexandria
cd alexandria
cargo build --release
# binaries at target/release/alexandria, alexandria-mcp, alexandria-brain
```

Run the test suite:

```bash
cargo test
```

## Quickstart

### Use with the Codex app

The fastest way to use Alexandria as a second brain is to register it as an MCP server in the OpenAI Codex app (Desktop or IDE extension).

1. Build the binaries and initialize a library:

```bash
cargo build --release
./target/release/alexandria init ~/alexandria   # or any directory you like
```

2. In the Codex app, open the MCP server settings (gear icon → Codex Settings) and **Add** a **STDIO** server:

| Field | Value |
| --- | --- |
| Name | `Alexandria` |
| Command to launch | `/absolute/path/to/alexandria/target/release/alexandria-mcp` |
| Argument 1 | `--library` |
| Argument 2 | `/absolute/path/to/your-library` |

> **Use absolute paths.** The app launches the binary directly (no shell), so `~` is **not** expanded — `--library ~/alexandria` will fail to find the library and the server exits on launch (looks like "nothing happens"). Use e.g. `/Users/you/alexandria`.

3. Install the memory skill so the agent follows the recall → act → remember loop:

```bash
alexandria-brain init ~/alexandria   # writes the bundled SKILL.md
mkdir -p ~/.codex/skills/alexandria-memory
cp ~/alexandria/.alexandria/codex/skills/alexandria-memory/SKILL.md \
   ~/.codex/skills/alexandria-memory/SKILL.md
```

4. Restart the Codex app. In a thread, mention `$alexandria-memory`, then ask the agent *"What MCP tools do you have?"* to confirm `recall`, `remember`, `expand`, `coverage`, `survey`, `map`, etc. are available.

> The app may not list the server under `/mcp` or the MCP settings panel even when it works — a known display quirk. Confirm by asking the agent directly.

For the full Codex integration (including the `alexandria-brain` CLI loop and sandbox notes), see [docs/SECOND_BRAIN.md](docs/SECOND_BRAIN.md).

### CLI

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

# Record when a source was observed (ISO8601); observation/document/repo sources
# default to now if --observed is omitted
alexandria remember "Hatco repo inspected" --source observation:repo/hatco \
  --observed 2026-05-29T18:00:00Z

# 3. Orient and recall
alexandria catalog                              # collections + tags with counts
alexandria recall "hybrid retrieval"
alexandria recall "auth jwt" --budget 1500 --format json

# Structured/faceted recall: scope to collections/tags for deterministic,
# enumerable results when fuzzy matching is ambiguous (composes with the query).
# recall also auto-detects matching facets and suggests scoping in human output.
alexandria recall "preferences" --collection project-x
alexandria recall "anything" --tag auth --format json

# Memory self-awareness: how much do we know, and where is the depth?
alexandria coverage "Hatco"                     # density x-ray + recommended_next
alexandria survey "Hatco" --budget 3000         # exhaustive claims + body token costs
alexandria map eng_7f3a2c --depth 2              # concept graph from id or topic
alexandria map "Cartographer Agent" --rel depends_on --format json

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

## Second-brain loop (Codex)

Alexandria's memory engine is fully usable on its own. Optionally, run a packaged **second-brain loop** that drives OpenAI Codex as the agent with Alexandria wired in via MCP:

```bash
# Install + authenticate the Codex CLI separately (see docs/SECOND_BRAIN.md)

# Provision library + Codex MCP config + alexandria-memory skill
alexandria-brain init

# Run a task (Codex recalls before acting, remembers after; brain consolidates post-turn)
alexandria-brain run "What did we decide about auth?"
alexandria-brain run "Research X and remember findings" --sandbox read-only --format json
```

`cargo build --release` produces three binaries: `alexandria` (standalone CLI), `alexandria-mcp` (stdio MCP server), and `alexandria-brain` (orchestrator). Put `alexandria-mcp` on PATH so Codex can spawn it.

Because memory writes go through the MCP server (not Codex's file sandbox), `--sandbox read-only` still persists memory while preventing workspace file edits.

Full setup, sandbox notes, and troubleshooting: [docs/SECOND_BRAIN.md](docs/SECOND_BRAIN.md).

### Shared remote memory (one brain for all your agents)

`alexandria-mcp` can also serve over **HTTP**, so a single server becomes shared memory for every MCP-capable agent — Codex, Claude, Cursor — connecting by URL. One store, one index, one embedding space.

The repo ships with an **OAuth proxy** (`proxy/`) in front of `alexandria-mcp` so different clients can authenticate the way they expect:

```
  Claude web ── OAuth (DCR + PKCE) ──┐
  Cursor/Codex ── static bearer ───┼──▶ alexandria-oauth-proxy :8081 ──▶ alexandria-mcp :8080
                                     │         (TLS via Caddy / Cloudflare / …)
```

| Client | Auth | What you configure |
| --- | --- | --- |
| **Claude web** (Connectors) | OAuth 2.1 — Claude registers dynamically (DCR), you sign in via browser | MCP URL only: `https://your-domain/mcp` — leave Client ID / secret empty |
| **Cursor, Codex, Claude Desktop/API** | Static bearer (when `ALLOW_LEGACY_STATIC_TOKEN=true`) | Same URL + `Authorization: Bearer <ALEXANDRIA_MCP_TOKEN>` |

The proxy validates the client's OAuth JWT or legacy bearer, then injects `ALEXANDRIA_MCP_TOKEN` toward `alexandria-mcp`. The upstream server is unchanged.

**Quick start (Docker):**

```bash
cp proxy/.env.example .env   # or create .env — see docs/REMOTE.md
# Required in .env:
#   ALEXANDRIA_MCP_TOKEN=$(openssl rand -hex 32)
#   RESOURCE_URL=https://memory.example.com
#   LOGIN_PASSWORD=...        # browser login for Claude OAuth

alexandria init ./library     # if you haven't already
docker compose up -d --build  # alexandria-mcp (internal) + oauth-proxy on :8081
```

Put TLS in front of `:8081` (Caddy, nginx, Cloudflare Tunnel, …). Only the proxy is published to the host; `alexandria-mcp` stays on the internal Docker network.

**Claude web:** Settings → Connectors → Add custom connector → URL `https://your-domain/mcp`. When prompted, sign in with `LOGIN_USERNAME` / `LOGIN_PASSWORD` from `.env`.

**Cursor / Codex:** point at the same URL with your static token (see [docs/REMOTE.md](docs/REMOTE.md) for per-client config snippets).

Proxy internals, env reference, and verification commands: [proxy/README.md](proxy/README.md). Full remote deployment guide: [docs/REMOTE.md](docs/REMOTE.md).

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

When `recall.auto_facet` is enabled (default), matching collections/tags are detected from the query and blended additively. JSON output includes `detected_facets`; hits may carry a `freshness_warning` when source observation times exceed `[freshness].stale_after_days`.

### Self-awareness verbs

Use these when a top-k `recall` isn't enough context about *what* the library holds on a topic:

| Verb | Purpose |
| --- | --- |
| `catalog` | Global table of contents: collections and tags with counts |
| `coverage <topic>` | Memory-density x-ray: counts, provenance depth, recency, detail ratio, `recommended_next` |
| `survey <topic>` | Exhaustive-but-budgeted traversal with `body_tokens` / `expansion_value` per hit |
| `map <id\|topic>` | Typed-edge concept graph, budget-trimmed, with mermaid in human output |

`coverage` answers "how dense is my memory here?" `survey` answers "show me everything relevant cheaply and where the bodies hide." `map` answers "how do these concepts relate?"

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
embedder = "fastembed"     # "fastembed" (local), "ollama", "openai", "hash" (offline/tests)
# completer = "ollama"     # "ollama", "openai", "anthropic" — used by consolidation/shape

[providers.embedding]
# model = "BGESmallENV15"  # fastembed model name

[providers.ollama]
# base_url = "http://localhost:11434"
# embed_model = "nomic-embed-text"
# complete_model = "llama3"

[providers.openai]
# base_url = "https://api.openai.com/v1"
# embed_model = "text-embedding-3-small"
# complete_model = "gpt-4o-mini"
# api_key_env = "OPENAI_API_KEY"    # env var holding the key

[providers.anthropic]
# complete_model = "claude-3-5-haiku-20241022"
# api_key_env = "ANTHROPIC_API_KEY"

[reranker]
# enabled = false           # set to true to activate fastembed cross-encoder reranker
# model = "JINARerankerV1TurboEn"

[calibration]
# enabled = true
# score_weight_floor = 0.5  # min multiplier when domain reliability is weak

[recall]
# auto_facet = true           # detect matching collections/tags from query (additive boost)

[freshness]
# enabled = true
# stale_after_days = 30     # warn when youngest source observed is older than this

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

- `crates/core` — the library: `store` (plain-text truth), `index` (SQLite/FTS5 + sqlite-vec), `retrieval` (hybrid RRF + five-state recall + context tree + posture judge + facet detection), `graph` (traversal/`trace`/`timeline`), `coverage`/`survey`/`map` (memory self-awareness), `consolidate` (slow + fast passes), `meta` (meta-memory), `freshness`, `shape`, `style`, `threads`, `ops`, `provider` (`Embedder` / `Completer`), `config`, `engram`.
- `crates/cli` — the `alexandria` binary (built on `clap`).
- `crates/mcp` — `alexandria-mcp`: rmcp stdio MCP server exposing memory verbs as tools.
- `crates/brain` — `alexandria-brain`: Codex second-brain loop (`init` + `run` with post-turn consolidation).

Embeddings and LLM calls sit behind pluggable, synchronous provider traits with a local-first default. The default `fastembed` provider downloads an ONNX model on first use (~130MB); set `embedder = "hash"` for fully offline operation. HTTP providers (Ollama, OpenAI) cache the embedding dimension in `index_meta` and skip the probe call on re-open. `expand` does not load the embedder.

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for the complete design, including hybrid retrieval, progressive disclosure, consolidation, the conflict taxonomy, meta-memory, and response modes.

## Roadmap

| Milestone | Scope |
| --- | --- |
| **M1 — Skeleton** ✅ | Plain-text store, SQLite + FTS5 index, `init`/`remember`/`recall` (lexical)/`reindex`, five-state recall + response modes |
| **M2 — Hybrid + budget** ✅ | Local embeddings (`fastembed` + `hash` for tests), semantic search, RRF fusion, density-based gap states, progressive-disclosure context tree, `expand` |
| **M3 — Graph + consolidation** ✅ | Typed edges + traversal, conflict taxonomy, provenance (`--source`/`--derived-from` + `trace`), provisional promotion ladder, `link`/`timeline`/`archive`, the `reflect`/`consolidate` "sleep" pass |
| **M4 — Relational, shape, meta-memory, modes** ✅ | Relational `style` channel, episodic shape index, meta-memory (`meta`), response modes (`--audit`/`--high-stakes`), fast/slow reflection (`reflect --fast`), open-thread surfacing (`--surface-when` / `threads --surface-for`) |
| **M5 — Providers & polish** ✅ | Ollama + cloud providers (OpenAI, Anthropic), local reranker, meta-driven bounded self-calibration, sync provider traits, dim-probe caching |
| **Self-awareness** ✅ | `coverage` / `survey` / `map`, facet-aware recall, source freshness warnings |
| **Codex loop** ✅ | `alexandria-mcp` (MCP tools), `alexandria-brain` (Codex orchestrator + `alexandria-memory` skill) |

## License

MIT
