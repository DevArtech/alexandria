#!/usr/bin/env node
import http from "node:http";
import { timingSafeEqual } from "node:crypto";
import httpProxy from "http-proxy";

import { loadConfig } from "./config.js";
import { createLogger } from "./logger.js";
import { AuthError } from "./auth.js";
import { createStore } from "./store.js";
import { createOAuthServer } from "./oauth.js";
import { createRateLimiter, clientIp } from "./ratelimit.js";

async function main() {
  const config = loadConfig();
  const log = createLogger(config.logLevel);
  const store = await createStore(config.dataDir);
  const oauth = createOAuthServer(config, store, log);

  const loginLimiter = createRateLimiter({
    windowMs: config.rateLimitWindowMs,
    max: config.loginRateLimitMax,
  });
  const registerLimiter = createRateLimiter({
    windowMs: config.rateLimitWindowMs,
    max: config.registerRateLimitMax,
  });

  const proxy = httpProxy.createProxyServer({
    target: config.upstreamUrl,
    changeOrigin: true,
    xfwd: true,
    proxyTimeout: config.proxyTimeoutMs,
  });

  proxy.on("error", (err, req, res) => {
    log.error("upstream proxy error", { path: req?.url, message: err?.message });
    if (res && !res.headersSent) {
      res.writeHead(502, { "content-type": "application/json" });
    }
    if (res && !res.writableEnded) {
      res.end(JSON.stringify({ error: "bad_gateway", message: "upstream unreachable" }));
    }
  });

  proxy.on("proxyReq", (proxyReq) => {
    if (config.upstreamToken) {
      proxyReq.setHeader("authorization", `Bearer ${config.upstreamToken}`);
    } else {
      proxyReq.removeHeader("authorization");
    }
  });

  const server = http.createServer((req, res) => {
    handle(req, res, { config, log, oauth, proxy, loginLimiter, registerLimiter }).catch((err) => {
      log.error("unhandled request error", { message: err?.message });
      sendError(res, 500, "internal_error", "internal error");
    });
  });

  // Slowloris protection: bound how long a client may take to send headers and
  // the full request. 0 disables (not recommended on the public internet).
  server.headersTimeout = config.headersTimeoutMs;
  server.requestTimeout = config.requestTimeoutMs;

  server.listen(config.port, config.host, () => {
    log.info(`alexandria-oauth-proxy listening on http://${config.host}:${config.port}`);
    log.info(`issuer=${oauth.issuer}`);
    log.info(`proxying authenticated traffic to ${config.upstreamUrl}`);
    if (config.allowLegacyStaticToken) {
      log.warn("dual auth mode enabled: accepting legacy static bearer token");
    }
  });

  for (const sig of ["SIGINT", "SIGTERM"]) {
    process.on(sig, () => {
      log.info(`received ${sig}, shutting down`);
      server.close(() => process.exit(0));
    });
  }
}

async function handle(req, res, ctx) {
  const { config, log, oauth, proxy, loginLimiter, registerLimiter } = ctx;
  const url = new URL(req.url, config.resourceUrl || `http://${req.headers.host}`);
  const path = url.pathname;
  const protectedMetaPrefix = "/.well-known/oauth-protected-resource";
  const authMetaPrefix = "/.well-known/oauth-authorization-server";
  const oidcMetaPrefix = "/.well-known/openid-configuration";

  if (path === "/health" || path === "/healthz") {
    return sendJson(res, 200, { status: "ok" });
  }

  // Throttle the unauthenticated, abuse-prone endpoints before doing any work
  // (including body parsing) on them.
  if (req.method === "POST" && (path === "/login" || path === "/oauth/register")) {
    const limiter = path === "/login" ? loginLimiter : registerLimiter;
    const { allowed, retryAfterSec } = limiter.check(clientIp(req, config.trustProxy));
    if (!allowed) {
      log.warn("rate limited", { path, ip: clientIp(req, config.trustProxy) });
      res.setHeader("Retry-After", String(retryAfterSec));
      return sendError(res, 429, "too_many_requests", "rate limit exceeded, slow down");
    }
  }

  if (path === protectedMetaPrefix || path.startsWith(`${protectedMetaPrefix}/`)) {
    const suffix = path.slice(protectedMetaPrefix.length);
    return sendJson(res, 200, oauth.protectedResourceMetadata(suffix));
  }

  if (path === authMetaPrefix || path.startsWith(`${authMetaPrefix}/`)) {
    return sendJson(res, 200, oauth.metadata());
  }
  if (path === oidcMetaPrefix || path.startsWith(`${oidcMetaPrefix}/`)) {
    return sendJson(res, 200, oauth.metadata());
  }

  const oauthHandled = await oauth.handle(req, res, url);
  if (oauthHandled !== false) return;

  if (path === "/mcp" || path.startsWith("/oauth")) {
    log.info("request", { method: req.method, path, ua: req.headers["user-agent"]?.slice(0, 80) });
  }

  // Never let a client smuggle identity. We are the sole authority for these
  // headers; strip any inbound copies before authenticating and forwarding.
  stripInboundAuthHeaders(req);

  const token = bearerToken(req);
  if (!token) {
    return sendAuthChallenge(res, config, null, path);
  }

  if (config.allowLegacyStaticToken && constantTimeEq(token, config.legacyBearerToken)) {
    req.headers["x-auth-method"] = "legacy_bearer";
    return proxy.web(req, res);
  }

  let claims;
  try {
    claims = await oauth.verifyToken(token);
  } catch (err) {
    if (err instanceof AuthError) {
      log.warn("token rejected", { code: err.code, reason: err.description, path });
      return sendAuthChallenge(res, config, err, path);
    }
    throw err;
  }

  log.debug("authorized", { sub: claims.sub, path });
  req.headers["x-auth-method"] = "oauth_jwt";
  if (claims.sub) req.headers["x-auth-subject"] = String(claims.sub);
  if (claims.client_id) req.headers["x-auth-client-id"] = String(claims.client_id);

  proxy.web(req, res);
}

function stripInboundAuthHeaders(req) {
  for (const name of Object.keys(req.headers)) {
    if (name.toLowerCase().startsWith("x-auth-")) {
      delete req.headers[name];
    }
  }
}

function bearerToken(req) {
  const h = req.headers["authorization"];
  if (!h) return null;
  const m = /^Bearer\s+(.+)$/i.exec(h);
  return m ? m[1].trim() : null;
}

function constantTimeEq(a, b) {
  if (!a || !b) return false;
  const aa = Buffer.from(String(a));
  const bb = Buffer.from(String(b));
  if (aa.length !== bb.length) return false;
  return timingSafeEqual(aa, bb);
}

function sendAuthChallenge(res, config, authError, requestPath = "") {
  const params = [];
  if (config.resourceUrl) {
    const pathSuffix =
      requestPath && requestPath !== "/" && !requestPath.startsWith("/.well-known/")
        ? requestPath
        : config.mcpPath || "/mcp";
    const metaUrl =
      config.resourceUrl.replace(/\/+$/, "") + "/.well-known/oauth-protected-resource" + pathSuffix;
    params.push(`resource_metadata="${metaUrl}"`);
  }
  if (authError) {
    params.push(`error="${authError.code}"`);
    params.push(`error_description="${authError.description}"`);
  }
  // RFC 7235: "Bearer" followed by auth-params separated by commas (space after scheme).
  res.setHeader("WWW-Authenticate", `Bearer ${params.join(", ")}`);
  if (authError) {
    return sendError(res, authError.status || 401, authError.code, authError.description);
  }
  return sendError(res, 401, "unauthorized", "authentication required");
}

function sendJson(res, status, body) {
  const payload = JSON.stringify(body);
  res.writeHead(status, {
    "content-type": "application/json",
    "content-length": Buffer.byteLength(payload),
  });
  res.end(payload);
}

function sendError(res, status, code, message) {
  return sendJson(res, status, { error: code, message });
}

main().catch((err) => {
  process.stderr.write(`fatal: ${err?.stack || err?.message || err}\n`);
  process.exit(1);
});
