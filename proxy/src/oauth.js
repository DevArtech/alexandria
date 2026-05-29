import { createHash, randomBytes, randomUUID, timingSafeEqual } from "node:crypto";
import { importJWK, SignJWT } from "jose";
import { AuthError } from "./auth.js";

const DEFAULT_SCOPES = ["alexandria:read", "alexandria:write"];

export function createOAuthServer(config, store, log) {
  const issuer = config.issuer.replace(/\/+$/, "") + "/";
  const base = issuer.replace(/\/+$/, "");

  setInterval(() => store.gc(), 60_000).unref();

  return {
    issuer,
    metadata: () => authorizationServerMetadata(config, base),
    protectedResourceMetadata: (suffix = "") => protectedResourceMetadata(config, suffix),
    handle,
    verifyToken,
  };

  async function handle(req, res, url) {
    const path = url.pathname;

    if (path === "/.well-known/jwks.json" || path === "/oauth/jwks") {
      return sendJson(res, 200, { keys: [store.keys.publicJwk] });
    }
    if (path === "/oauth/register" && req.method === "POST") {
      return handleRegister(req, res);
    }
    if (path === "/authorize") {
      if (req.method === "GET") return handleAuthorizeGet(req, res, url);
      if (req.method === "POST") return handleAuthorizePost(req, res, url);
    }
    if (path === "/oauth/token" && req.method === "POST") {
      return handleToken(req, res);
    }
    if (path === "/login" && req.method === "GET") {
      return sendHtml(res, 200, loginPage(url.searchParams.get("return") || "/authorize"));
    }
    if (path === "/login" && req.method === "POST") {
      return handleLogin(req, res);
    }

    return false;
  }

  async function handleRegister(req, res) {
    let body;
    try {
      body = await readJsonBody(req);
    } catch {
      return oauthError(res, 400, "invalid_client_metadata", "invalid JSON body");
    }

    const redirectUris = body.redirect_uris;
    if (!Array.isArray(redirectUris) || !redirectUris.length) {
      return oauthError(res, 400, "invalid_redirect_uri", "redirect_uris required");
    }
    for (const uri of redirectUris) {
      if (!isAllowedRedirect(uri)) {
        return oauthError(res, 400, "invalid_redirect_uri", `redirect not allowed: ${uri}`);
      }
    }

    const clientId = randomUUID().replace(/-/g, "");
    const client = {
      client_id: clientId,
      client_id_issued_at: Math.floor(Date.now() / 1000),
      token_endpoint_auth_method: "none",
      grant_types: ["authorization_code", "refresh_token"],
      response_types: ["code"],
      redirect_uris: redirectUris,
      client_name: body.client_name || "MCP Client",
      scope: normalizeScope(body.scope),
    };
    await store.saveClient(client);
    log.info("client registered", { client_id: clientId, name: client.client_name });
    return sendJson(res, 201, client);
  }

  async function handleAuthorizeGet(req, res, url) {
    const params = url.searchParams;
    const err = validateAuthorizeParams(params);
    if (err) return oauthError(res, 400, err.code, err.message);

    const session = sessionFromReq(req);
    if (!session) {
      const ret = url.pathname + url.search;
      res.writeHead(302, { location: `/login?return=${encodeURIComponent(ret)}` });
      res.end();
      return;
    }

    return issueAuthCodeRedirect(res, params, session.sub);
  }

  async function handleAuthorizePost(req, res, url) {
    const form = await readFormBody(req);
    const params = url.searchParams;
    for (const [k, v] of form.entries()) params.set(k, v);

    const err = validateAuthorizeParams(params);
    if (err) return oauthError(res, 400, err.code, err.message);

    const session = sessionFromReq(req);
    if (!session) {
      return oauthError(res, 401, "login_required", "login required");
    }

    return issueAuthCodeRedirect(res, params, session.sub);
  }

  function issueAuthCodeRedirect(res, params, sub) {
    const code = randomBytes(32).toString("base64url");
    store.putAuthCode(code, {
      clientId: params.get("client_id"),
      redirectUri: params.get("redirect_uri"),
      scope: params.get("scope") || DEFAULT_SCOPES.join(" "),
      resource: params.get("resource") || config.mcpResourceUrl || config.audience,
      codeChallenge: params.get("code_challenge"),
      codeChallengeMethod: params.get("code_challenge_method") || "S256",
      sub,
      exp: Date.now() + config.authCodeTtlSec * 1000,
    });

    const loc = new URL(params.get("redirect_uri"));
    loc.searchParams.set("code", code);
    const state = params.get("state");
    if (state) loc.searchParams.set("state", state);
    res.writeHead(302, { location: loc.toString() });
    res.end();
  }

  async function handleLogin(req, res) {
    const form = await readFormBody(req);
    const username = form.get("username") || "";
    const password = form.get("password") || "";
    const ret = form.get("return") || "/authorize";

    if (
      !constantTimeEq(username, config.loginUsername) ||
      !constantTimeEq(password, config.loginPassword)
    ) {
      return sendHtml(res, 401, loginPage(ret, "Invalid username or password"));
    }

    const sid = randomBytes(24).toString("base64url");
    store.putSession(sid, {
      sub: username,
      exp: Date.now() + config.sessionTtlSec * 1000,
    });
    const secureCookie = config.resourceUrl?.startsWith("https:") ?? true;
    res.writeHead(302, {
      location: ret,
      "set-cookie": sessionCookie(config.sessionCookieName, sid, config.sessionTtlSec, secureCookie),
    });
    res.end();
  }

  async function handleToken(req, res) {
    const form = await readFormBody(req);
    const grant = form.get("grant_type");

    if (grant === "authorization_code") {
      return tokenAuthorizationCode(res, form);
    }
    if (grant === "refresh_token") {
      return oauthError(res, 400, "unsupported_grant_type", "refresh_token not implemented yet");
    }
    return oauthError(res, 400, "unsupported_grant_type", "unsupported grant_type");
  }

  async function tokenAuthorizationCode(res, form) {
    const code = form.get("code");
    const redirectUri = form.get("redirect_uri");
    const clientId = form.get("client_id");
    const codeVerifier = form.get("code_verifier");

    if (!code || !redirectUri || !clientId || !codeVerifier) {
      return oauthError(res, 400, "invalid_request", "missing required parameters");
    }

    const client = store.getClient(clientId);
    if (!client) {
      return oauthError(res, 401, "invalid_client", "unknown client");
    }
    if (!client.redirect_uris.includes(redirectUri)) {
      return oauthError(res, 400, "invalid_grant", "redirect_uri mismatch");
    }

    const rec = store.takeAuthCode(code);
    if (!rec || rec.clientId !== clientId || rec.redirectUri !== redirectUri) {
      return oauthError(res, 400, "invalid_grant", "invalid or expired code");
    }

    if (!verifyPkce(codeVerifier, rec.codeChallenge, rec.codeChallengeMethod)) {
      return oauthError(res, 400, "invalid_grant", "PKCE verification failed");
    }

    const accessToken = await mintAccessToken({
      sub: rec.sub,
      clientId,
      scope: rec.scope,
      audience: rec.resource || config.mcpResourceUrl || config.audience,
    });

    return sendJson(res, 200, {
      access_token: accessToken,
      token_type: "Bearer",
      expires_in: config.accessTokenTtlSec,
      scope: rec.scope,
    });
  }

  async function mintAccessToken({ sub, clientId, scope, audience }) {
    const key = await importJWK(store.keys.privateJwk, "RS256");
    const aud = audience || config.mcpResourceUrl || config.audience;
    return new SignJWT({ scope, client_id: clientId })
      .setProtectedHeader({ alg: "RS256", kid: store.keys.kid, typ: "JWT" })
      .setIssuer(issuer)
      .setSubject(sub)
      .setAudience(aud)
      .setIssuedAt()
      .setExpirationTime(`${config.accessTokenTtlSec}s`)
      .sign(key);
  }

  async function verifyToken(token) {
    const { jwtVerify } = await import("jose");
    const key = await importJWK(store.keys.publicJwk, "RS256");
    const audiences = [
      config.mcpResourceUrl,
      config.resourceUrl,
      config.audience,
    ].filter(Boolean);
    try {
      const { payload } = await jwtVerify(token, key, {
        issuer,
        audience: audiences.length ? audiences : undefined,
        clockTolerance: config.clockToleranceSec,
      });
      assertScopes(payload, config.requiredScopes);
      return payload;
    } catch (err) {
      if (err?.code === "ERR_JWT_EXPIRED") {
        throw new AuthError("invalid_token", "token expired");
      }
      if (err?.code === "ERR_JWT_CLAIM_VALIDATION_FAILED") {
        throw new AuthError("invalid_token", `claim check failed: ${err.claim}`);
      }
      throw new AuthError("invalid_token", "invalid token");
    }
  }

  function validateAuthorizeParams(params) {
    if (params.get("response_type") !== "code") {
      return { code: "unsupported_response_type", message: "only code supported" };
    }
    const clientId = params.get("client_id");
    const redirectUri = params.get("redirect_uri");
    const codeChallenge = params.get("code_challenge");
    if (!clientId || !redirectUri || !codeChallenge) {
      return { code: "invalid_request", message: "missing required parameters" };
    }
    const client = store.getClient(clientId);
    if (!client) return { code: "unauthorized_client", message: "unknown client" };
    if (!client.redirect_uris.includes(redirectUri)) {
      return { code: "invalid_request", message: "invalid redirect_uri" };
    }
    const method = params.get("code_challenge_method") || "S256";
    if (method !== "S256") {
      return { code: "invalid_request", message: "only S256 PKCE supported" };
    }
    return null;
  }

  function sessionFromReq(req) {
    const cookie = parseCookies(req.headers.cookie || "")[config.sessionCookieName];
    if (!cookie) return null;
    return store.getSession(cookie);
  }

  function isAllowedRedirect(uri) {
    try {
      const u = new URL(uri);
      if (u.protocol !== "https:") return false;
      // Claude MCP callback and localhost dev.
      if (u.hostname === "claude.ai" && u.pathname.startsWith("/api/mcp/")) return true;
      if (u.hostname === "localhost" || u.hostname === "127.0.0.1") return true;
      return false;
    } catch {
      return false;
    }
  }
}

function authorizationServerMetadata(config, base) {
  return {
    issuer: config.issuer.replace(/\/+$/, "") + "/",
    authorization_endpoint: `${base}/authorize`,
    token_endpoint: `${base}/oauth/token`,
    registration_endpoint: `${base}/oauth/register`,
    jwks_uri: `${base}/.well-known/jwks.json`,
    response_types_supported: ["code"],
    grant_types_supported: ["authorization_code"],
    token_endpoint_auth_methods_supported: ["none"],
    code_challenge_methods_supported: ["S256"],
    scopes_supported: config.scopes.length ? config.scopes : DEFAULT_SCOPES,
  };
}

function protectedResourceMetadata(config, resourceSuffix = "") {
  const suffix = resourceSuffix && resourceSuffix.startsWith("/") ? resourceSuffix : "";
  const resource =
    !suffix || suffix === config.mcpPath
      ? config.mcpResourceUrl
      : `${config.resourceUrl.replace(/\/+$/, "")}${suffix}`;
  return {
    resource,
    authorization_servers: [config.issuer.replace(/\/+$/, "") + "/"],
    scopes_supported: config.scopes.length ? config.scopes : DEFAULT_SCOPES,
    bearer_methods_supported: ["header"],
  };
}

function verifyPkce(verifier, challenge, method) {
  if (method !== "S256") return false;
  const digest = createHash("sha256").update(verifier).digest("base64url");
  return constantTimeEq(digest, challenge);
}

function assertScopes(payload, requiredScopes) {
  if (!requiredScopes.length) return;
  const raw = payload.scope ?? payload.scp ?? "";
  const granted = new Set(
    (Array.isArray(raw) ? raw.join(" ") : String(raw)).split(/\s+/).filter(Boolean),
  );
  const missing = requiredScopes.filter((s) => !granted.has(s));
  if (missing.length) {
    throw new AuthError("insufficient_scope", `missing required scope(s): ${missing.join(", ")}`, 403);
  }
}

function normalizeScope(scope) {
  if (!scope) return DEFAULT_SCOPES.join(" ");
  return String(scope).trim() || DEFAULT_SCOPES.join(" ");
}

function sessionCookie(name, sid, maxAgeSec, secure = true) {
  const flags = ["HttpOnly", "SameSite=Lax", "Path=/", `Max-Age=${maxAgeSec}`];
  if (secure) flags.unshift("Secure");
  return `${name}=${sid}; ${flags.join("; ")}`;
}

function parseCookies(header) {
  const out = {};
  for (const part of header.split(";")) {
    const i = part.indexOf("=");
    if (i === -1) continue;
    out[part.slice(0, i).trim()] = decodeURIComponent(part.slice(i + 1).trim());
  }
  return out;
}

function loginPage(returnUrl, error = "") {
  const err = error ? `<p style="color:#b00020">${escapeHtml(error)}</p>` : "";
  return `<!doctype html><html><head><meta charset="utf-8"><title>Alexandria Login</title></head>
<body style="font-family:system-ui,sans-serif;max-width:420px;margin:4rem auto;padding:1rem">
<h1>Alexandria</h1><p>Sign in to authorize MCP access.</p>${err}
<form method="POST" action="/login">
<input type="hidden" name="return" value="${escapeHtml(returnUrl)}"/>
<label>Username<br/><input name="username" required autocomplete="username"/></label><br/><br/>
<label>Password<br/><input name="password" type="password" required autocomplete="current-password"/></label><br/><br/>
<button type="submit">Sign in</button>
</form></body></html>`;
}

function escapeHtml(s) {
  return String(s)
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

function constantTimeEq(a, b) {
  const aa = Buffer.from(String(a));
  const bb = Buffer.from(String(b));
  if (aa.length !== bb.length) return false;
  return timingSafeEqual(aa, bb);
}

async function readJsonBody(req) {
  const raw = await readBody(req);
  return JSON.parse(raw.toString("utf8"));
}

async function readFormBody(req) {
  const raw = await readBody(req);
  return new URLSearchParams(raw.toString("utf8"));
}

function readBody(req) {
  return new Promise((resolve, reject) => {
    const chunks = [];
    req.on("data", (c) => chunks.push(c));
    req.on("end", () => resolve(Buffer.concat(chunks)));
    req.on("error", reject);
  });
}

function sendJson(res, status, body) {
  const payload = JSON.stringify(body);
  res.writeHead(status, {
    "content-type": "application/json",
    "content-length": Buffer.byteLength(payload),
  });
  res.end(payload);
}

function sendHtml(res, status, html) {
  res.writeHead(status, { "content-type": "text/html; charset=utf-8" });
  res.end(html);
}

function oauthError(res, status, error, description) {
  return sendJson(res, status, { error, error_description: description });
}
