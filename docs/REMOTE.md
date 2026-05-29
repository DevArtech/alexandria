# Alexandria as a shared remote memory (one brain for all your agents)

Run a single `alexandria-mcp` server over HTTP and connect every MCP-capable
agent — OpenAI Codex, Claude, Cursor, etc. — to the **same** memory by URL.
All agents read and write one store, one index, one embedding space.

```
                      ┌─────────────────────────────────────────────────────┐
  Claude web ────────▶│  TLS (Caddy / Cloudflare Tunnel / …)                │
  (OAuth DCR+PKCE)    │         │                                           │
                      │         ▼                                           │
  Cursor/Codex ──────▶│  alexandria-oauth-proxy :8081                       │
  (static bearer)     │    · OAuth AS (DCR, /authorize, /login, /token)     │
                      │    · optional legacy bearer passthrough             │
                      │    · injects ALEXANDRIA_MCP_TOKEN upstream          │
                      │         │                                           │
                      │         ▼                                           │
                      │  alexandria-mcp :8080  (internal, not on host)      │
                      │         │                                           │
                      │         ▼                                           │
                      │  alexandria-core: store + index                     │
                      │         │                                           │
                      │  embedder ──▶ local OpenAI-compatible endpoint      │
                      └─────────────────────────────────────────────────────┘
```

Why one remote server (not a synced folder): there is exactly **one embedder**,
so every client shares an identical vector space, and concurrent writes are
serialized safely against the single SQLite index.

Why the OAuth proxy: **Claude web** requires OAuth for remote MCP connectors
(DCR + PKCE). It does not accept user-pasted bearer tokens. The proxy is a thin
Node reverse proxy that acts as a self-hosted authorization server and forwards
authenticated traffic to `alexandria-mcp`, which still enforces its own static
`ALEXANDRIA_MCP_TOKEN`. Other clients (Cursor, Codex, Claude Desktop) can keep
using the static bearer via optional dual-auth mode.

Proxy source and env reference: [proxy/README.md](../proxy/README.md).

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
- The `openai` provider **omits the Authorization header when no key is set**,
  so keyless local servers (Ollama, LocalAI, text-embeddings-inference) work
  out of the box.
- `fastembed` (local, in-process) also works and needs no endpoint (included in
  the Dockerfile).
- All clients must agree on the embedder, which is automatic here since only the
  **server** embeds. If you ever change the embedder, run `alexandria reindex`.

---

## 2. Configure and run (Docker)

Create a repo-root `.env` (see [proxy/.env.example](../proxy/.env.example)):

```bash
# Static token alexandria-mcp expects (proxy injects this after client auth)
ALEXANDRIA_MCP_TOKEN=$(openssl rand -hex 32)

# Public HTTPS URL of the proxy (no trailing path — MCP is at /mcp)
RESOURCE_URL=https://memory.example.com

# Browser login for Claude OAuth authorization
LOGIN_USERNAME=admin
LOGIN_PASSWORD=$(openssl rand -hex 16)

# Let Cursor/Codex use the static bearer alongside OAuth
ALLOW_LEGACY_STATIC_TOKEN=true
```

Start the stack:

```bash
docker compose up -d --build
```

This starts:
- **`alexandria-mcp`** on `:8080` — internal only; bearer-auth via `ALEXANDRIA_MCP_TOKEN`
- **`alexandria-oauth-proxy`** on `127.0.0.1:8081` — the only service bound to the host

Put a TLS terminator in front of `:8081` (Caddy, nginx, Traefik, Cloudflare Tunnel, …).

Example Cloudflare Tunnel ingress:

```yaml
ingress:
  - hostname: memory.example.com
    service: http://127.0.0.1:8081
```

### Verify

```bash
# Liveness (no auth)
curl https://memory.example.com/health

# OAuth discovery
curl https://memory.example.com/.well-known/oauth-protected-resource/mcp
curl https://memory.example.com/.well-known/oauth-authorization-server

# MCP without token → 401 + WWW-Authenticate (Claude uses this to start OAuth)
curl -i -X POST https://memory.example.com/mcp \
  -H 'content-type: application/json' \
  -H 'accept: application/json, text/event-stream' \
  -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"t","version":"1"}}}'

# MCP with static bearer (dual-auth mode)
curl -X POST https://memory.example.com/mcp \
  -H "authorization: Bearer $ALEXANDRIA_MCP_TOKEN" \
  -H 'content-type: application/json' \
  -H 'accept: application/json, text/event-stream' \
  -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"t","version":"1"}}}'
```

End-to-end OAuth smoke test (DCR → login → token → `/mcp`):

```bash
cd proxy && npm install   # once, for the script only
LOGIN_USERNAME=admin LOGIN_PASSWORD=... \
  node scripts/oauth-smoke-test.mjs https://memory.example.com
```

### Running without Docker

Run both processes on the host (or only the proxy if `alexandria-mcp` is elsewhere):

```bash
# Terminal 1 — MCP server (bind loopback; proxy is the public face)
ALEXANDRIA_MCP_TOKEN=$(openssl rand -hex 32) \
  alexandria-mcp --transport http --bind 127.0.0.1:8080 --library /srv/alexandria

# Terminal 2 — OAuth proxy
cd proxy && cp .env.example .env   # edit RESOURCE_URL, LOGIN_PASSWORD, tokens
npm install && npm start
```

Then TLS-terminate `:8081` as above.

---

## 3. Connect your agents

All clients use the **proxy URL**: `https://memory.example.com/mcp`.

### Claude web (Connectors)

Claude web requires OAuth and performs Dynamic Client Registration automatically.

1. Settings → Connectors → **Add custom connector**
2. **URL:** `https://memory.example.com/mcp`
3. **Client ID / Client secret:** leave **empty** — Claude registers its own client via DCR
4. Connect → browser login opens → sign in with `LOGIN_USERNAME` / `LOGIN_PASSWORD`

Claude's OAuth callback is `https://claude.ai/api/mcp/auth_callback` (already allowed by the proxy).

If the connector spins on "Checking connection…", delete it and re-add with **only the URL** (no client ID/secret). Ensure the proxy returns a proper `401` with:

```http
WWW-Authenticate: Bearer resource_metadata="https://memory.example.com/.well-known/oauth-protected-resource/mcp"
```

### OpenAI Codex — `~/.codex/config.toml`

Requires `ALLOW_LEGACY_STATIC_TOKEN=true` in the proxy `.env`.

```toml
[mcp_servers.alexandria]
url = "https://memory.example.com/mcp"
bearer_token_env_var = "ALEXANDRIA_MCP_TOKEN"
enabled = true
```

Set `ALEXANDRIA_MCP_TOKEN` in your environment to the same value as in the server `.env`.

### Cursor — `~/.cursor/mcp.json`

```json
{
  "mcpServers": {
    "alexandria": {
      "url": "https://memory.example.com/mcp",
      "headers": { "Authorization": "Bearer <YOUR_ALEXANDRIA_MCP_TOKEN>" }
    }
  }
}
```

### Claude Desktop — custom connector

URL `https://memory.example.com/mcp` with `Authorization: Bearer <token>` header
(if your Desktop build supports remote MCP with bearer auth). For stdio-only setups,
use `mcp-remote` as a bridge.

### Anthropic API — MCP connector

```json
{
  "model": "claude-sonnet-4-5",
  "mcp_servers": [
    {
      "type": "url",
      "url": "https://memory.example.com/mcp",
      "name": "alexandria",
      "authorization_token": "<YOUR_ALEXANDRIA_MCP_TOKEN>"
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

## 4. OAuth proxy configuration

All proxy settings are environment variables. In Docker Compose these live in the
repo-root `.env` and are passed to `alexandria-oauth-proxy`.

| Variable | Required | Description |
| --- | --- | --- |
| `RESOURCE_URL` | yes | Public HTTPS origin of the proxy (e.g. `https://memory.example.com`) |
| `LOGIN_PASSWORD` | yes | Password for the browser login form |
| `ALEXANDRIA_MCP_TOKEN` | yes | Static bearer injected toward `alexandria-mcp` |
| `LOGIN_USERNAME` | no | Login username (default `admin`) |
| `ALLOW_LEGACY_STATIC_TOKEN` | no | If `true`, accept static bearer from clients (default `false`) |
| `OAUTH_AUDIENCE` | no | JWT audience (defaults to `RESOURCE_URL/mcp`) |
| `OAUTH_SCOPES` | no | Advertised scopes (default `alexandria:read alexandria:write`) |

Signing keys and DCR-registered clients persist in the `oauth_proxy_data` Docker
volume (`DATA_DIR=/data`). Back up this volume if you rely on long-lived DCR
clients; keys are auto-generated on first start.

---

## 5. Tools exposed

`recall`, `expand`, `remember`, `link`, `trace`, `timeline`, `threads`, `style`,
`meta`, `archive`, `consolidate` — identical to the CLI verbs and the stdio
server. Relational memory is structurally suppressed in `recall`/`expand`/`style`
regardless of transport.

---

## 6. Operating notes

- **Backups**: the source of truth is the plain-text `library/` (and `meta_log/`).
  Back it up (or `git` it). `index.db` is a rebuildable cache (`alexandria reindex`).
- **Concurrency**: a single server process serializes all access via an internal
  lock, so simultaneous writes from different agents are safe.
- **Auth layers**: clients authenticate to the **proxy** (OAuth JWT or legacy bearer).
  The proxy authenticates to **alexandria-mcp** with `ALEXANDRIA_MCP_TOKEN`.
  Rotate the static token if leaked; OAuth users re-authorize via the browser login.
- **Firewall**: if you allowlist inbound traffic, Anthropic's MCP proxy egress range
  is `160.79.104.0/21` (for Claude web connectors).
- **Embedding endpoint**: keep it reachable from the `alexandria-mcp` container
  (same compose network, or `host.docker.internal` for a host service).

---

## 7. Troubleshooting

| Symptom | Likely cause | Fix |
| --- | --- | --- |
| Claude spins on "Checking connection…" | Malformed `WWW-Authenticate`, or client ID/secret filled in incorrectly | Re-add connector with URL only; verify `curl -i POST …/mcp` returns `Bearer resource_metadata=…` |
| Claude never opens login | Discovery failure (metadata 404, wrong `resource` field) | Check `/.well-known/oauth-protected-resource/mcp` returns `"resource": "https://…/mcp"` |
| 502 from public URL | TLS terminator points at wrong port | Tunnel/proxy should target `:8081`, not `:8080` |
| Cursor/Codex 401 | Dual-auth disabled or wrong token | Set `ALLOW_LEGACY_STATIC_TOKEN=true`; use same `ALEXANDRIA_MCP_TOKEN` |
| OAuth login works but MCP 401 | Token audience mismatch | Set `OAUTH_AUDIENCE=https://your-domain/mcp` |

Proxy logs: `docker compose logs -f alexandria-oauth-proxy`
