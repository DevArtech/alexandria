import { mkdir, readFile, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { generateKeyPair, exportJWK } from "jose";

// File-backed persistence for signing keys and dynamically registered clients.
export async function createStore(dataDir) {
  await mkdir(dataDir, { recursive: true });
  const keysPath = join(dataDir, "keys.json");
  const clientsPath = join(dataDir, "clients.json");

  let keys = await readJson(keysPath);
  if (!keys?.privateJwk || !keys?.publicJwk) {
    const { publicKey, privateKey } = await generateKeyPair("RS256", { modulusLength: 2048 });
    const publicJwk = await exportJWK(publicKey);
    const privateJwk = await exportJWK(privateKey);
    publicJwk.alg = "RS256";
    publicJwk.use = "sig";
    privateJwk.alg = "RS256";
    keys = { privateJwk, publicJwk, kid: publicJwk.kid || "alexandria-1" };
    publicJwk.kid = keys.kid;
    privateJwk.kid = keys.kid;
    await writeJson(keysPath, keys);
  }

  let clients = await readJson(clientsPath);
  if (!clients || typeof clients !== "object") clients = {};

  const authCodes = new Map(); // code -> { clientId, redirectUri, scope, resource, codeChallenge, sub, exp }
  const sessions = new Map(); // sessionId -> { sub, exp }

  return {
    keys,
    getClient(id) {
      return clients[id] || null;
    },
    saveClient(client) {
      clients[client.client_id] = client;
      return persistClients();
    },
    listClients() {
      return Object.values(clients);
    },
    putAuthCode(code, record) {
      authCodes.set(code, record);
    },
    takeAuthCode(code) {
      const rec = authCodes.get(code);
      if (!rec) return null;
      authCodes.delete(code);
      if (rec.exp < Date.now()) return null;
      return rec;
    },
    putSession(id, record) {
      sessions.set(id, record);
    },
    getSession(id) {
      const rec = sessions.get(id);
      if (!rec) return null;
      if (rec.exp < Date.now()) {
        sessions.delete(id);
        return null;
      }
      return rec;
    },
    deleteSession(id) {
      sessions.delete(id);
    },
    gc() {
      const now = Date.now();
      for (const [k, v] of authCodes) {
        if (v.exp < now) authCodes.delete(k);
      }
      for (const [k, v] of sessions) {
        if (v.exp < now) sessions.delete(k);
      }
    },
  };

  async function persistClients() {
    await writeJson(clientsPath, clients);
  }
}

async function readJson(path) {
  try {
    const raw = await readFile(path, "utf8");
    return JSON.parse(raw);
  } catch {
    return null;
  }
}

async function writeJson(path, data) {
  await writeFile(path, JSON.stringify(data, null, 2) + "\n", "utf8");
}
