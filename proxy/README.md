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
- **Browser login**: simple username/password form at `/login` (credentials from env), with a
  double-submit CSRF token and `/logout` for explicit session revocation.
- **Abuse hardening**: per-IP rate limiting on `/login` and `/oauth/register`, request
  header/body timeouts, a capped request-body size, and stripping of inbound `x-auth-*` headers.
- **Dual auth mode** (optional): accept legacy static bearer alongside OAuth JWTs (defaults to
  `ALEXANDRIA_MCP_TOKEN` when `LEGACY_BEARER_TOKEN` is unset).
- **Rewrites auth**: strips the client token and injects the upstream static bearer.
- **Streams**: long-lived Streamable-HTTP / SSE responses pass straight through.
- **Discovery**: RFC 9728 protected resource metadata + RFC 8414 authorization server metadata.
- **Health**: `/health` and `/healthz` are unauthenticated.

## Configuration

All configuration is via environment variables (see [`.env.example`](./.env.example)).

| Variable                    | Required    | Default                            | Description                                                                               |
| --------------------------- | ----------- | ---------------------------------- | ----------------------------------------------------------------------------------------- |
| `RESOURCE_URL`              | yes         | –                                  | Public HTTPS URL of this proxy (issuer + resource metadata).                              |
| `LOGIN_PASSWORD`            | yes         | –                                  | Password for the browser login form.                                                      |
| `OAUTH_MODE`                | no          | `self`                             | `self` = built-in authorization server.                                                   |
| `OAUTH_ISSUER`              | no          | `RESOURCE_URL`                     | Token `iss` claim and metadata issuer.                                                    |
| `OAUTH_AUDIENCE`            | no          | `RESOURCE_URL`                     | Expected token audience.                                                                  |
| `LOGIN_USERNAME`            | no          | `admin`                            | Login form username.                                                                      |
| `DATA_DIR`                  | no          | `/data`                            | Persist signing keys + DCR clients.                                                       |
| `OAUTH_SCOPES`              | no          | `alexandria:read alexandria:write` | Advertised scopes.                                                                        |
| `OAUTH_REQUIRED_SCOPES`     | no          | –                                  | Scopes every request must carry.                                                          |
| `ALLOW_LEGACY_STATIC_TOKEN` | no          | `false`                            | Also accept legacy static bearer.                                                         |
| `LEGACY_BEARER_TOKEN`       | no          | `ALEXANDRIA_MCP_TOKEN`             | Legacy client token for Cursor/Codex; leave empty to reuse the MCP token.                 |
| `UPSTREAM_URL`              | no          | `http://127.0.0.1:8080`            | `alexandria-mcp` base URL.                                                                |
| `ALEXANDRIA_MCP_TOKEN`      | recommended | –                                  | Static bearer injected upstream.                                                          |
| `TRUST_PROXY`               | no          | `false`                            | Trust `X-Forwarded-For` for client IP (enable only behind a trusted TLS terminator).      |
| `MAX_BODY_BYTES`            | no          | `65536`                            | Max request body on OAuth/login endpoints; `0` disables.                                  |
| `HEADERS_TIMEOUT_MS`        | no          | `60000`                            | Max time to receive request headers (Slowloris guard); `0` disables.                      |
| `REQUEST_TIMEOUT_MS`        | no          | `120000`                           | Max time to receive the full request; `0` disables.                                       |
| `PROXY_TIMEOUT_MS`          | no          | `0`                                | Upstream socket timeout. `0` keeps long-lived SSE alive; set finite if upstream can hang. |
| `RATE_LIMIT_WINDOW_MS`      | no          | `900000`                           | Window for per-IP rate limiting (15 min).                                                 |
| `LOGIN_RATE_LIMIT_MAX`      | no          | `10`                               | Max `POST /login` attempts per IP per window.                                             |
| `REGISTER_RATE_LIMIT_MAX`   | no          | `20`                               | Max `POST /oauth/register` per IP per window.                                             |
| `ALLOW_LOCALHOST_REDIRECTS` | no          | `false`                            | Allow loopback (`localhost`/`127.0.0.1`/`::1`) redirect URIs for dev clients.             |
| `CSRF_COOKIE_NAME`          | no          | `alexandria_csrf`                  | Cookie name for the login form CSRF token.                                                |
| `PORT` / `HOST`             | no          | `8081` / `0.0.0.0`                 | Listener.                                                                                 |
| `LOG_LEVEL`                 | no          | `info`                             | `error`/`warn`/`info`/`debug`.                                                            |

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
