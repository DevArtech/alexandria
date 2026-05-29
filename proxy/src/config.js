// Centralized, validated configuration sourced from the environment.

function req(name) {
  const v = process.env[name];
  if (!v || !v.trim()) {
    throw new Error(`Missing required environment variable: ${name}`);
  }
  return v.trim();
}

function opt(name, fallback) {
  const v = process.env[name];
  return v && v.trim() ? v.trim() : fallback;
}

function bool(name, fallback = false) {
  const v = opt(name, "");
  if (!v) return fallback;
  return /^(1|true|yes|on)$/i.test(v);
}

function list(name) {
  return opt(name, "")
    .split(/[\s,]+/)
    .map((s) => s.trim())
    .filter(Boolean);
}

export function loadConfig(env = process.env) {
  const prev = process.env;
  process.env = env;
  try {
    const mode = opt("OAUTH_MODE", "self").toLowerCase();
    const resourceUrl = opt("RESOURCE_URL");
    const issuer = opt("OAUTH_ISSUER", resourceUrl);
    if (!issuer) {
      throw new Error("Set RESOURCE_URL (public URL of this proxy).");
    }

    const upstreamToken = opt("ALEXANDRIA_MCP_TOKEN");
    const allowLegacyStaticToken = bool("ALLOW_LEGACY_STATIC_TOKEN", false);
    const legacyBearerToken = opt("LEGACY_BEARER_TOKEN", upstreamToken);
    if (allowLegacyStaticToken && !legacyBearerToken) {
      throw new Error(
        "ALLOW_LEGACY_STATIC_TOKEN=true requires LEGACY_BEARER_TOKEN or ALEXANDRIA_MCP_TOKEN.",
      );
    }

    const scopes = list("OAUTH_SCOPES").length
      ? list("OAUTH_SCOPES")
      : ["alexandria:read", "alexandria:write"];

    const baseUrl = resourceUrl ? resourceUrl.replace(/\/+$/, "") : issuer.replace(/\/+$/, "");
    const mcpPath = opt("MCP_PATH", "/mcp").replace(/\/+$/, "") || "/mcp";
    const mcpResourceUrl = baseUrl.endsWith(mcpPath) ? baseUrl : `${baseUrl}${mcpPath}`;

    const cfg = {
      mode,
      port: Number(opt("PORT", "8081")),
      host: opt("HOST", "0.0.0.0"),
      upstreamUrl: opt("UPSTREAM_URL", "http://127.0.0.1:8080").replace(/\/+$/, ""),
      upstreamToken,
      allowLegacyStaticToken,
      legacyBearerToken,

      issuer: issuer.replace(/\/+$/, ""),
      resourceUrl: baseUrl,
      mcpPath,
      mcpResourceUrl,
      audience: opt("OAUTH_AUDIENCE", mcpResourceUrl).replace(/\/+$/, ""),
      scopes,
      requiredScopes: list("OAUTH_REQUIRED_SCOPES"),
      clockToleranceSec: Number(opt("OAUTH_CLOCK_TOLERANCE_SEC", "5")),

      dataDir: opt("DATA_DIR", "/data"),
      loginUsername: opt("LOGIN_USERNAME", "admin"),
      loginPassword: opt("LOGIN_PASSWORD", ""),
      sessionCookieName: opt("SESSION_COOKIE_NAME", "alexandria_session"),
      sessionTtlSec: Number(opt("SESSION_TTL_SEC", String(24 * 3600))),
      authCodeTtlSec: Number(opt("AUTH_CODE_TTL_SEC", "600")),
      accessTokenTtlSec: Number(opt("ACCESS_TOKEN_TTL_SEC", "3600")),

      logLevel: opt("LOG_LEVEL", "info"),
    };

    if (mode === "self" && !cfg.loginPassword) {
      throw new Error("Set LOGIN_PASSWORD for self-hosted OAuth login.");
    }

    return cfg;
  } finally {
    process.env = prev;
  }
}
