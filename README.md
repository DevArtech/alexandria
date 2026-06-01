# Alexandria

![Alexandria — agent-native memory substrate](Alexandria.png)

> A local-first, CLI-first "second brain" designed for how an LLM actually thinks, retrieves, and reasons — not for how a human files paper notes.

Named after the Library of Alexandria, this is a memory substrate built for an LLM consumer. Its prime directive is simple:

> **Maximize useful information per token, and let the agent control retrieval depth.**

Memory lives as plain-text Markdown files (the source of truth) with a rebuildable SQLite index layered on top. Nothing is locked in: delete the index and rebuild it from text at any time.

## The ethos

Most "AI memory" is just `chunk → embed → top-k cosine`. That discards structure, exact recall, relationships, recency, provenance, and — critically — the ability to say *"I think I know this but can't retrieve it cleanly."* Alexandria keeps semantic search as **one signal among several** inside a structured, typed, provenance-aware, uncertainty-aware system.

Three load-bearing principles:

- **Plain text is the source of truth.** Every index is a rebuildable cache.
- **Honest ignorance is a first-class outcome.** `recall` returns one of five states, not just "rows or nothing."
- **Enforce by structure, not convention.** When a constraint matters (e.g. relational memory never being quoted), it's made impossible to violate.

Memory is typed into tiers, each with its own lifecycle — **working** (ephemeral), **episodic** (append-only events), **provisional** (usable but unearned), **semantic** (curated facts), **procedural** (how-tos), and **relational** (how to work with a user; shapes generation only, **never** quoted). The atomic unit is an **Engram**: a Markdown file with structured YAML frontmatter (id, tier, status, claim, provenance, confidence, salience, typed links).

The full design — hybrid retrieval, progressive disclosure, consolidation, the conflict taxonomy, meta-memory, and response modes — is in **[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)**.

## Quick start (local)

### Build

Requires a recent stable [Rust](https://www.rust-lang.org/tools/install) toolchain.

```bash
git clone <repo-url> alexandria
cd alexandria
cargo build --release   # binaries: alexandria, alexandria-mcp, alexandria-brain
cargo test              # run the suite
```

Put the binaries on your `PATH` (Codex/Cursor spawn `alexandria-mcp` directly, so it must be discoverable):

```bash
cp target/release/alexandria target/release/alexandria-mcp target/release/alexandria-brain ~/.local/bin/
```

`alexandria` is the standalone CLI, `alexandria-mcp` is the stdio/HTTP MCP server, and `alexandria-brain` is the Codex second-brain orchestrator.

> **Contributing?** Run `./scripts/setup-hooks.sh` once to enable the pre-commit gate. It auto-formats and lints what it can (`cargo fmt`, `cargo clippy --fix`, `prettier`, `eslint --fix`), then blocks the commit unless `cargo fmt`/`clippy`/`cargo test` and the proxy's `prettier`/`eslint`/typecheck all pass. The same checks run in CI ([`.github/workflows/ci.yml`](.github/workflows/ci.yml)) on every push and pull request.
>
> **Releases.** Pushes to `main` run the release workflow ([`.github/workflows/release.yml`](.github/workflows/release.yml)). If the workspace Cargo version maps to a new tag (for example `0.1.1` -> `v0.1.1`), GitHub Actions builds the release binaries with `cargo build --workspace --release --locked` and publishes a GitHub Release containing `alexandria`, `alexandria-mcp`, and `alexandria-brain`.

### Use Alexandria directly (CLI)

A library is just a directory — `git init` it for free time-travel over your memory.

```bash
# Initialize a library in the current directory
alexandria init

# Remember something (first line becomes the claim, the rest the body)
alexandria remember "Alexandria uses hybrid fused retrieval, not vector-only"
cat notes.md | alexandria remember -                       # pipe longer content

# Tag, file into a collection, or set the tier/status
alexandria remember "The user prefers terse answers" --tier relational
alexandria remember "Auth flow uses short-lived JWTs" --collection project-x --tag auth

# Record provenance: where a claim came from, or what it was derived from
alexandria remember "User said use Rust" --tier episodic --source conversation:conv_1
alexandria remember "Alexandria is written in Rust" --derived-from eng_89187aa4

# Orient and recall (hybrid five-signal retrieval, token-budgeted)
alexandria catalog                              # collections + tags with counts
alexandria recall "hybrid retrieval"
alexandria recall "auth jwt" --budget 1500 --format json
alexandria recall "preferences" --collection project-x   # scope to a facet

# Memory self-awareness: how much do we know, and where is the depth?
alexandria coverage "Hatco"                     # density x-ray + recommended_next
alexandria survey "Hatco" --budget 3000         # exhaustive claims + body token costs
alexandria map "Cartographer Agent" --rel depends_on --format json   # concept graph

# Expand a hit to full body + linked claims; relate engrams with typed edges
alexandria expand eng_7f3a2c
alexandria link eng_new supersedes eng_old      # old is marked superseded + archived

# Walk provenance, view the timeline, surface open threads
alexandria trace eng_7f3a2c
alexandria timeline --since 2026-05-01 --tier episodic
alexandria threads --surface-for database

# Maintain: archive (never deleted), consolidate, reflect, rebuild
alexandria archive eng_old                      # alias: alexandria forget eng_old
alexandria consolidate                          # dedupe, promote/demote, decay, re-summarize
alexandria reflect --fast                       # quick, non-canonical briefing
alexandria reindex                              # rebuild the index from the Markdown store
```

Every command accepts `--format json` for machine/agent consumption and `--library <path>` to target a specific library (otherwise Alexandria discovers the nearest `.alexandria/` from the current directory upward).

**What `recall` returns.** Rather than always returning rows, `recall` classifies the result so an agent can adopt an honest posture, and attaches a recommended **response mode** (`flow` / `humility` / `audit`):

| State | Meaning |
| --- | --- |
| `strong_hit` | High-confidence, discriminating match |
| `weak_hit` | Something matched, but low confidence — hedge |
| `high_confidence_gap` | Relevant memory likely exists but can't be surfaced cleanly |
| `low_confidence_gap` | Topic is adjacent to known domains; nothing precise |
| `nothing` | No meaningful signal |

### Use Alexandria with a local agent (MCP)

`alexandria-mcp` exposes the memory verbs as MCP tools over stdio, so any MCP-capable agent can recall and remember.

**Codex app / Cursor (stdio MCP).** Register `alexandria-mcp` as a STDIO server pointing at your library:

```jsonc
// Codex app: gear → Codex Settings → Add STDIO server, or Cursor ~/.cursor/mcp.json
{
  "command": "/absolute/path/to/alexandria-mcp",
  "args": ["--library", "/absolute/path/to/your-library"]
}
```

> **Use absolute paths.** The app launches the binary directly (no shell), so `~` is **not** expanded — `--library ~/alexandria` will fail to find the library and the server exits on launch.

Install the bundled memory skill so the agent follows the recall → act → remember loop:

```bash
alexandria-brain init ~/alexandria   # writes the bundled SKILL.md
mkdir -p ~/.codex/skills/alexandria-memory
cp ~/alexandria/.alexandria/codex/skills/alexandria-memory/SKILL.md \
   ~/.codex/skills/alexandria-memory/SKILL.md
```

Restart the app, mention `$alexandria-memory` in a thread, then ask the agent *"What MCP tools do you have?"* to confirm `recall`, `remember`, `expand`, `coverage`, `survey`, `map`, etc. are available.

**Second-brain loop (Codex).** Optionally, run the packaged loop that drives OpenAI Codex with Alexandria wired in via MCP and consolidates after each turn:

```bash
# Install + authenticate the Codex CLI separately first
alexandria-brain init                                   # provision library + Codex config + skill
alexandria-brain run "What did we decide about auth?"
alexandria-brain run "Research X and remember findings" --sandbox read-only --format json
```

Because memory writes go through the MCP server (not Codex's file sandbox), `--sandbox read-only` still persists memory while preventing workspace file edits. Full setup, internals, sandbox notes, and troubleshooting: **[docs/SECOND_BRAIN.md](docs/SECOND_BRAIN.md)**.

## Shared memory across devices and agents

`alexandria-mcp` can also serve over **HTTP**, so a single server becomes shared memory for every MCP-capable agent — Codex, Claude, Cursor — connecting by URL. One store, one index, one embedding space (only the server embeds, so every client shares an identical vector space, and concurrent writes are serialized against the single SQLite index).

The repo ships an **OAuth proxy** (`proxy/`) in front of `alexandria-mcp` so different clients authenticate the way they expect:

```
  Claude web ── OAuth (DCR + PKCE) ──┐
  Cursor/Codex ── static bearer ─────┼──▶ alexandria-oauth-proxy :8081 ──▶ alexandria-mcp :8080
                                     │         (TLS via Caddy / Cloudflare / …)
```

| Client | Auth | What you configure |
| --- | --- | --- |
| **Claude web** (Connectors) | OAuth 2.1 — Claude registers dynamically (DCR), you sign in via browser | MCP URL only: `https://your-domain/mcp` — leave Client ID / secret empty |
| **Cursor, Codex, Claude Desktop/API** | Static bearer (when `ALLOW_LEGACY_STATIC_TOKEN=true`) | Same URL + `Authorization: Bearer <ALEXANDRIA_MCP_TOKEN>` |

> **Prefer the static bearer token whenever a client supports it.** OAuth access tokens expire, so browser-OAuth clients (Claude web) periodically force a re-sign-in. A static `Authorization: Bearer <ALEXANDRIA_MCP_TOKEN>` is long-lived and avoids those short-term sign-outs — use it for Cursor, Codex, Claude Desktop/API (with `ALLOW_LEGACY_STATIC_TOKEN=true`), and reserve OAuth for clients like Claude web that require it.

**Quick start (Docker):**

```bash
cp proxy/.env.example .env   # then set, at minimum:
#   ALEXANDRIA_MCP_TOKEN=$(openssl rand -hex 32)
#   RESOURCE_URL=https://memory.example.com
#   LOGIN_PASSWORD=...        # browser login for Claude OAuth

alexandria init ./library     # if you haven't already
docker compose up -d --build  # alexandria-mcp (internal) + oauth-proxy on :8081
```

Put TLS in front of `:8081` (Caddy, nginx, Cloudflare Tunnel, …). Only the proxy is published to the host; `alexandria-mcp` stays on the internal Docker network. Then point each agent at `https://your-domain/mcp` — Claude web via Connectors (URL only), Cursor/Codex with the static bearer.

Full deployment guide (embedder choice, per-client config, verification, operating notes): **[docs/REMOTE.md](docs/REMOTE.md)**. Proxy internals and env reference: **[proxy/README.md](proxy/README.md)**.

## Configuration

`.alexandria/config.toml` is created on `init`:

```toml
[providers]
embedder = "fastembed"     # "fastembed" (local), "ollama", "openai", "hash" (offline/tests)
# completer = "ollama"     # "ollama", "openai", "anthropic" — used by consolidation/shape

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
# enabled = false           # set true to activate the local fastembed cross-encoder
# model = "JINARerankerV1TurboEn"

[calibration]
# enabled = true
# score_weight_floor = 0.5  # min multiplier when domain reliability is weak

[recall]
# auto_facet = true         # detect matching collections/tags from query (additive boost)

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

**Tuning notes:**

- Distance thresholds are L2 distances in embedding space and **must be tuned per embedder**. The defaults are oriented to `fastembed`; the `hash` embedder's distances are much larger (roughly `weak ≈ 1.25`, `centroid ≈ 1.4`, `density ≈ 1.55`).
- For the gap states to be reachable, keep the ordering **`semantic_weak_max_distance < centroid_radius < density_radius`** — otherwise a query can never be "far from any clean hit yet inside a dense neighborhood."
- The default `fastembed` embedder downloads an ONNX model (~130 MB) on first use; set `embedder = "hash"` for fully offline operation (no semantic-quality guarantees).

### Library layout

```
my-library/
├── .alexandria/
│   ├── config.toml         # providers, budgets, thresholds
│   ├── index.db            # SQLite cache (FTS5 + sqlite-vec + …) — rebuildable, git-ignored
│   ├── meta_log/           # append-only meta-memory events — survives reindex
│   ├── fast_reflections/   # non-canonical fast-pass briefings (never scanned as memory)
│   └── codex/              # isolated CODEX_HOME (MCP config + skills) when using brain
├── episodic/   provisional/   semantic/   procedural/
├── relational/             # never surfaced as quotable text
├── threads/                # open threads (unresolved_by_design)
├── collections/            # roll-up summaries written by `consolidate`
└── archive/                # "forgotten" / superseded — moved here, never deleted
```

`.alexandria/` holds only derived/config data; everything else is canonical text. See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for the full memory model and storage substrate.

## Roadmap

All milestones below are **complete** — Alexandria is fully implemented through M5 plus post-M5 polish.

| Milestone | Scope | Status |
| --- | --- | --- |
| **M1 — Skeleton** | Plain-text store, SQLite + FTS5 index, `init`/`remember`/`recall` (lexical)/`reindex`, five-state recall + response modes | ✅ |
| **M2 — Hybrid + budget** | Local embeddings (`fastembed` + `hash` for tests), semantic search, RRF fusion, density-based gap states, progressive-disclosure context tree, `expand` | ✅ |
| **M3 — Graph + consolidation** | Typed edges + traversal, conflict taxonomy, provenance (`--source`/`--derived-from` + `trace`), provisional promotion ladder, `link`/`timeline`/`archive`, the `reflect`/`consolidate` "sleep" pass | ✅ |
| **M4 — Relational, shape, meta-memory, modes** | Relational `style` channel, episodic shape index, meta-memory (`meta`), response modes (`--audit`/`--high-stakes`), fast/slow reflection (`reflect --fast`), open-thread surfacing | ✅ |
| **M5 — Providers & polish** | Ollama + cloud providers (OpenAI, Anthropic), local reranker, meta-driven bounded self-calibration, sync provider traits, dim-probe caching | ✅ |
| **Self-awareness** | `coverage` / `survey` / `map`, facet-aware recall, source freshness warnings | ✅ |
| **Codex loop** | `alexandria-mcp` (MCP tools), `alexandria-brain` (Codex orchestrator + `alexandria-memory` skill), shared remote memory over HTTP + OAuth proxy | ✅ |

**Deliberate deferrals (not bugs):** meta-memory signals are operator-driven (`meta --record-correction` / `--record-gap`) rather than auto-detected from conversation; self-calibration is bounded score down-weighting in low-reliability domains, not full per-domain threshold self-tuning. See the open questions in [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md#16-open-questions--milestones).

## License

MIT
