# Alexandria as a shared remote memory (one brain for all your agents)

Run a single `alexandria-mcp` server over HTTP and connect every MCP-capable
agent — OpenAI Codex, Claude (Desktop or API), Cursor, etc. — to the **same**
memory by URL. All agents read and write one store, one index, one embedding
space.

```
                      ┌──────────────────────────────────────┐
  Codex ─┐            │  your server                          │
  Claude ─┤  HTTPS    │  Caddy (TLS) ──▶ alexandria-mcp :8080 │
  Cursor ─┼──────────▶│                     │                 │
  …      ─┘  + bearer │                     ▼                 │
                      │        alexandria-core: store + index │
                      │                     │                 │
                      │     embedder ──▶ local OpenAI-compatible
                      │                  endpoint (Ollama/LocalAI/TEI)
                      └──────────────────────────────────────┘
```

Why one remote server (not a synced folder): there is exactly **one embedder**,
so every client shares an identical vector space, and concurrent writes are
serialized safely against the single SQLite index. A shared filesystem with
per-machine servers would risk index corruption and embedding-space mismatches.

---

## 1. Prepare the library

On the server, create the library and choose the embedder. For a self-hosted,
keyless, OpenAI-compatible endpoint (recommended for shared infra):

```bash
alexandria init ./library
```

Edit `./library/.alexandria/config.toml`:

```toml
[providers]
embedder = "openai"          # the "openai" provider speaks the OpenAI wire format

[providers.openai]
base_url = "http://ollama:11434/v1"   # your local endpoint (Ollama shown)
embed_model = "nomic-embed-text"       # a model your endpoint serves
api_key_env = "OPENAI_API_KEY"         # leave unset for keyless local servers
```

Notes:
- The `openai` provider now **omits the Authorization header when no key is set**,
  so keyless local servers (Ollama, LocalAI, text-embeddings-inference, Infinity)
  work out of the box. Set `OPENAI_API_KEY` only if your endpoint requires it.
- `fastembed` (local, in-process) also works and needs no endpoint, but the
  container then needs the ONNX runtime libs (already included in the Dockerfile).
- All clients must agree on the embedder, which is automatic here since only the
  **server** embeds. If you ever change the embedder, run `alexandria reindex`.

---

## 2. Run the server (Docker + Caddy)

```bash
cp .env.example .env
# edit .env: ALEXANDRIA_DOMAIN, ALEXANDRIA_MCP_TOKEN (openssl rand -hex 32)

docker compose up -d --build
```

This starts:
- `alexandria-mcp` on `:8080` (HTTP, bearer-auth) — not published to the host
- `caddy` on `:80/:443`, auto-TLS for `ALEXANDRIA_DOMAIN`, proxying to the server

Verify:

```bash
curl https://memory.example.com/health           # -> ok
curl -X POST https://memory.example.com/mcp \
  -H 'authorization: Bearer <YOUR_TOKEN>' \
  -H 'content-type: application/json' \
  -H 'accept: application/json, text/event-stream' \
  -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"t","version":"1"}}}'
# 401 without the header; a JSON/SSE result with it.
```

### Running without Docker

```bash
ALEXANDRIA_MCP_TOKEN=$(openssl rand -hex 32) \
  alexandria-mcp --transport http --bind 127.0.0.1:8080 --library /srv/alexandria
```

Then put any TLS-terminating reverse proxy (Caddy/nginx/Traefik) in front. The
server enforces the bearer token itself; if `ALEXANDRIA_MCP_TOKEN` is unset it
runs **unauthenticated** (only acceptable behind a trusted proxy that adds auth).

---

## 3. Connect your agents

All clients point at `https://memory.example.com/mcp` with an
`Authorization: Bearer <token>` header.

### OpenAI Codex (CLI or app) — `~/.codex/config.toml`

```toml
[mcp_servers.alexandria]
url = "https://memory.example.com/mcp"
bearer_token_env_var = "ALEXANDRIA_MCP_TOKEN"   # set this env var for Codex
enabled = true
```

(Or inline the header if your Codex version supports `http_headers`.)

### Cursor — `~/.cursor/mcp.json` (or project `.cursor/mcp.json`)

```json
{
  "mcpServers": {
    "alexandria": {
      "url": "https://memory.example.com/mcp",
      "headers": { "Authorization": "Bearer <YOUR_TOKEN>" }
    }
  }
}
```

### Claude Desktop — custom connector

Settings → Connectors → Add custom connector → URL
`https://memory.example.com/mcp`, with an `Authorization: Bearer <token>` header.
(Claude Desktop's remote-MCP connector support; for stdio-only builds, use
`mcp-remote` as a bridge.)

### Anthropic API — MCP connector

```json
{
  "model": "claude-sonnet-4-5",
  "mcp_servers": [
    {
      "type": "url",
      "url": "https://memory.example.com/mcp",
      "name": "alexandria",
      "authorization_token": "<YOUR_TOKEN>"
    }
  ],
  "messages": [{ "role": "user", "content": "What do we know about project X?" }]
}
```

### The memory skill

So agents follow the recall→act→remember loop, install the `alexandria-memory`
skill where each client looks for skills (e.g. `~/.codex/skills/` for Codex), or
paste its guidance into the agent's system prompt. Generate the file with
`alexandria-brain init` and copy `…/.alexandria/codex/skills/alexandria-memory/SKILL.md`.

---

## 4. Tools exposed

`recall`, `expand`, `remember`, `link`, `trace`, `timeline`, `threads`, `style`,
`meta`, `archive`, `consolidate` — identical to the CLI verbs and the stdio
server. Relational memory is structurally suppressed in `recall`/`expand`/`style`
regardless of transport.

---

## 5. Operating notes

- **Backups**: the source of truth is the plain-text `library/` (and `meta_log/`).
  Back it up (or `git` it). `index.db` is a rebuildable cache (`alexandria reindex`).
- **Concurrency**: a single server process serializes all access via an internal
  lock, so simultaneous writes from different agents are safe.
- **Auth**: the bearer token is the only thing standing between the internet and
  your memory — use a long random value and rotate it if leaked. Prefer keeping
  the server bound to `127.0.0.1`/an internal network and exposing it only
  through the TLS proxy.
- **Embedding endpoint**: keep it reachable from the container (same compose
  network, or `host.docker.internal` for a host service).
