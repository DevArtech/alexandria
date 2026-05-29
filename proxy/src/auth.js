import { createRemoteJWKSet, jwtVerify, errors as joseErrors } from "jose";

// Resolve the JWKS URL: either provided directly, or discovered from the
// issuer's OIDC well-known document. Discovery is done once at startup.
async function resolveJwksUri(config) {
  if (config.jwksUri) return config.jwksUri;

  const discoveryUrl =
    config.issuer.replace(/\/+$/, "") + "/.well-known/openid-configuration";
  const res = await fetch(discoveryUrl, {
    headers: { accept: "application/json" },
  });
  if (!res.ok) {
    throw new Error(
      `OIDC discovery failed (${res.status}) at ${discoveryUrl}; ` +
        "set OAUTH_JWKS_URI explicitly.",
    );
  }
  const doc = await res.json();
  if (!doc.jwks_uri) {
    throw new Error(`OIDC discovery doc at ${discoveryUrl} has no jwks_uri.`);
  }
  return doc.jwks_uri;
}

// Build a token verifier. The returned function resolves to the verified
// payload, or throws an AuthError with an OAuth-style reason on failure.
export async function createVerifier(config, logger) {
  const jwksUri = await resolveJwksUri(config);
  logger.info(`jwks: ${jwksUri}`);

  const JWKS = createRemoteJWKSet(new URL(jwksUri), {
    // Cache keys and rate-limit refetches against key-rotation churn.
    cacheMaxAge: 10 * 60 * 1000,
    cooldownDuration: 30 * 1000,
  });

  const verifyOptions = {
    algorithms: config.algorithms,
    clockTolerance: config.clockToleranceSec,
  };
  if (config.issuer) verifyOptions.issuer = config.issuer;
  if (config.audiences.length) verifyOptions.audience = config.audiences;

  return async function verify(token) {
    let payload;
    try {
      ({ payload } = await jwtVerify(token, JWKS, verifyOptions));
    } catch (err) {
      throw toAuthError(err);
    }
    assertScopes(payload, config.requiredScopes);
    return payload;
  };
}

function assertScopes(payload, requiredScopes) {
  if (!requiredScopes.length) return;
  // OAuth scopes live in `scope` (space-delimited string) per RFC 8693/9068,
  // or `scp` (array or string) in some issuers (e.g. Azure AD).
  const raw = payload.scope ?? payload.scp ?? "";
  const granted = new Set(
    (Array.isArray(raw) ? raw.join(" ") : String(raw)).split(/\s+/).filter(Boolean),
  );
  const missing = requiredScopes.filter((s) => !granted.has(s));
  if (missing.length) {
    throw new AuthError(
      "insufficient_scope",
      `missing required scope(s): ${missing.join(", ")}`,
      403,
    );
  }
}

export class AuthError extends Error {
  constructor(code, description, status = 401) {
    super(description);
    this.name = "AuthError";
    this.code = code; // OAuth error code, e.g. "invalid_token"
    this.description = description;
    this.status = status;
  }
}

function toAuthError(err) {
  if (err instanceof joseErrors.JWTExpired) {
    return new AuthError("invalid_token", "token expired");
  }
  if (err instanceof joseErrors.JWTClaimValidationFailed) {
    return new AuthError("invalid_token", `claim check failed: ${err.claim}`);
  }
  if (
    err instanceof joseErrors.JWSSignatureVerificationFailed ||
    err instanceof joseErrors.JWKSNoMatchingKey
  ) {
    return new AuthError("invalid_token", "signature verification failed");
  }
  if (err instanceof joseErrors.JOSEError) {
    return new AuthError("invalid_token", "malformed token");
  }
  // Network/JWKS fetch failures are not the client's fault.
  return new AuthError("temporarily_unavailable", "unable to verify token", 503);
}
