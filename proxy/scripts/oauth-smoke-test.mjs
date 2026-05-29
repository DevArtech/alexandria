#!/usr/bin/env node
/**
 * End-to-end smoke test: DCR -> authorize -> login -> token -> /mcp
 * Usage: node scripts/oauth-smoke-test.mjs [baseUrl]
 */
import { createHash, randomBytes } from "node:crypto";

const base = (process.argv[2] || "http://127.0.0.1:8081").replace(/\/+$/, "");
const loginUser = process.env.LOGIN_USERNAME || "admin";
const loginPass = process.env.LOGIN_PASSWORD || "";

if (!loginPass) {
  console.error("Set LOGIN_PASSWORD in the environment.");
  process.exit(1);
}

function pkce() {
  const verifier = randomBytes(32).toString("base64url");
  const challenge = createHash("sha256").update(verifier).digest("base64url");
  return { verifier, challenge };
}

function parseSetCookie(res) {
  const raw = res.headers.getSetCookie?.() || [];
  const cookies = {};
  for (const line of raw) {
    const [pair] = line.split(";");
    const i = pair.indexOf("=");
    if (i === -1) continue;
    cookies[pair.slice(0, i).trim()] = pair.slice(i + 1).trim();
  }
  return cookies;
}

async function main() {
  console.log(`base=${base}`);

  const reg = await fetch(`${base}/oauth/register`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({
      client_name: "smoke-test",
      redirect_uris: ["https://claude.ai/api/mcp/auth_callback"],
    }),
  });
  if (!reg.ok) throw new Error(`DCR failed: ${reg.status} ${await reg.text()}`);
  const client = await reg.json();
  console.log("DCR ok", client.client_id);

  const { verifier, challenge } = pkce();
  const state = randomBytes(8).toString("hex");
  const authUrl = new URL(`${base}/authorize`);
  authUrl.searchParams.set("response_type", "code");
  authUrl.searchParams.set("client_id", client.client_id);
  authUrl.searchParams.set(
    "redirect_uri",
    "https://claude.ai/api/mcp/auth_callback",
  );
  authUrl.searchParams.set("scope", "alexandria:read alexandria:write");
  authUrl.searchParams.set("code_challenge", challenge);
  authUrl.searchParams.set("code_challenge_method", "S256");
  authUrl.searchParams.set("state", state);
  authUrl.searchParams.set("resource", "https://alexandria.artzima.dev");

  let cookies = {};
  let res = await fetch(authUrl, { redirect: "manual" });
  if (res.status !== 302) throw new Error(`authorize step1: expected 302, got ${res.status}`);
  const loginLoc = res.headers.get("location");
  console.log("authorize -> login", loginLoc);

  res = await fetch(`${base}${loginLoc}`, { redirect: "manual" });
  if (res.status !== 200) throw new Error(`login form: ${res.status}`);

  const body = new URLSearchParams({
    username: loginUser,
    password: loginPass,
    return: authUrl.pathname + authUrl.search,
  });
  res = await fetch(`${base}/login`, {
    method: "POST",
    headers: { "content-type": "application/x-www-form-urlencoded" },
    body,
    redirect: "manual",
  });
  Object.assign(cookies, parseSetCookie(res));
  if (res.status !== 302) throw new Error(`login POST: ${res.status} ${await res.text()}`);
  const afterLogin = res.headers.get("location");
  console.log("login ok ->", afterLogin);

  const cookieHeader = Object.entries(cookies)
    .map(([k, v]) => `${k}=${v}`)
    .join("; ");
  res = await fetch(`${base}${afterLogin}`, {
    redirect: "manual",
    headers: { cookie: cookieHeader },
  });
  if (res.status !== 302) throw new Error(`authorize step2: ${res.status}`);
  const cb = new URL(res.headers.get("location"));
  const code = cb.searchParams.get("code");
  if (!code) throw new Error("no code in redirect");
  console.log("auth code received");

  const tokenBody = new URLSearchParams({
    grant_type: "authorization_code",
    code,
    redirect_uri: "https://claude.ai/api/mcp/auth_callback",
    client_id: client.client_id,
    code_verifier: verifier,
  });
  res = await fetch(`${base}/oauth/token`, {
    method: "POST",
    headers: { "content-type": "application/x-www-form-urlencoded" },
    body: tokenBody,
  });
  if (!res.ok) throw new Error(`token: ${res.status} ${await res.text()}`);
  const token = await res.json();
  console.log("token ok", { expires_in: token.expires_in, scope: token.scope });

  res = await fetch(`${base}/mcp`, {
    method: "POST",
    headers: {
      authorization: `Bearer ${token.access_token}`,
      "content-type": "application/json",
      accept: "application/json, text/event-stream",
    },
    body: JSON.stringify({
      jsonrpc: "2.0",
      id: 1,
      method: "initialize",
      params: {
        protocolVersion: "2024-11-05",
        capabilities: {},
        clientInfo: { name: "oauth-smoke", version: "1" },
      },
    }),
  });
  const mcpText = await res.text();
  if (!res.ok) throw new Error(`/mcp: ${res.status} ${mcpText}`);
  console.log("/mcp ok", mcpText.slice(0, 200));

  res = await fetch(`${base}/.well-known/oauth-protected-resource/mcp`);
  const meta = await res.json();
  console.log("protected resource metadata", meta.resource);

  console.log("\nAll checks passed.");
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
