# alexandria-oauth-proxy

A thin Node reverse proxy in front of `alexandria-mcp` that adds **self-hosted OAuth 2.1**
(DCR + PKCE) for Claude web remote MCP, while optionally keeping a **legacy static bearer**
path for other clients (Cursor, Codex, etc.).

```
 OAuth JWT / legacy bearer              static bearer
 MCP client ─────────▶  alexandria-oauth-proxy ─────────▶ alexandria-mcp :8080
 (Claude web…)          · DCR + authorize + login         (unchanged)
                        · mint RS256 access tokens
                        · inject ALEXANDRIA_MCP_TOKEN
```

The upstream `alexandria-mcp` is unchanged: it still enforces its
`ALEXANDRIA_MCP_TOKEN`. The proxy converts a valid OAuth access token (or legacy
bearer, if enabled) into a trusted internal request.

## What it does

- **Self-hosted authorization server**: RFC 7591 DCR, authorization code + PKCE (S256),
  local RS256 JWT access tokens, JWKS at `/.well-known/jwks.json`.
- **Browser login**: simple username/password form at `/login` (credentials from env).
- **Dual auth mode** (optional): accept legacy static bearer alongside OAuth JWTs.
- **Rewrites auth**: strips the client token and injects the upstream static bearer.
- **Streams**: long-lived Streamable-HTTP / SSE responses pass straight through.
- **Discovery**: RFC 9728 protected resource metadata + RFC 8414 authorization server metadata.
- **Health**: `/health` and `/healthz` are unauthenticated.

## Configuration

All configuration is via environment variables (see [`.env.example`](./.env.example)).

| Variable | Required | Default | Description |
| --- | --- | --- | --- |
| `RESOURCE_URL` | yes | – | Public HTTPS URL of this proxy (issuer + resource metadata). |
| `LOGIN_PASSWORD` | yes | – | Password for the browser login form. |
| `OAUTH_MODE` | no | `self` | `self` = built-in authorization server. |
| `OAUTH_ISSUER` | no | `RESOURCE_URL` | Token `iss` claim and metadata issuer. |
| `OAUTH_AUDIENCE` | no | `RESOURCE_URL` | Expected token audience. |
| `LOGIN_USERNAME` | no | `admin` | Login form username. |
| `DATA_DIR` | no | `/data` | Persist signing keys + DCR clients. |
| `OAUTH_SCOPES` | no | `alexandria:read alexandria:write` | Advertised scopes. |
| `OAUTH_REQUIRED_SCOPES` | no | – | Scopes every request must carry. |
| `ALLOW_LEGACY_STATIC_TOKEN` | no | `false` | Also accept legacy static bearer. |
| `LEGACY_BEARER_TOKEN` | no | `ALEXANDRIA_MCP_TOKEN` | Legacy client token. |
| `UPSTREAM_URL` | no | `http://127.0.0.1:8080` | `alexandria-mcp` base URL. |
| `ALEXANDRIA_MCP_TOKEN` | recommended | – | Static bearer injected upstream. |
| `PORT` / `HOST` | no | `8081` / `0.0.0.0` | Listener. |
| `LOG_LEVEL` | no | `info` | `error`/`warn`/`info`/`debug`. |

## Run it

### Locally

```bash
cd proxy
npm install
cp .env.example .env   # edit RESOURCE_URL, LOGIN_PASSWORD, upstream token
set -a && . ./.env && set +a
npm start
```

### Docker (with Alexandria)

The repo's [`docker-compose.yml`](../docker-compose.yml) wires this proxy in
front of `alexandria-mcp`. Only the proxy is published to the host. Set vars in
the repo-root `.env`, then:

```bash
docker compose up -d --build
```

Put your TLS terminator (Caddy/nginx/Traefik/Cloudflare Tunnel) in front of `:8081`.

## Verify

```bash
# Liveness
curl localhost:8081/health

# Discovery
curl localhost:8081/.well-known/oauth-authorization-server
curl localhost:8081/.well-known/oauth-protected-resource/mcp

# DCR
curl -X POST localhost:8081/oauth/register \
  -H 'content-type: application/json' \
  -d '{"client_name":"test","redirect_uris":["https://claude.ai/api/mcp/auth_callback"]}'

# Missing token -> 401 with bearer challenge
curl -i -X POST localhost:8081/mcp
```

Full OAuth flow (authorize → login → token) requires a browser or scripted cookie handling.

## Client config

- **Claude web**: add remote MCP server URL `https://your-domain/mcp` — Claude uses DCR + OAuth automatically.
- **Cursor / Codex**: point at the proxy URL with `Authorization: Bearer <static token>` if `ALLOW_LEGACY_STATIC_TOKEN=true`.
