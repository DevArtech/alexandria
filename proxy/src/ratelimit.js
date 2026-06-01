// In-memory fixed-window rate limiter. The proxy is a single process, so a
// process-local map is sufficient; front with Cloudflare/Caddy for multi-replica
// or distributed limits.

export function createRateLimiter({ windowMs, max }) {
  const hits = new Map(); // key -> { count, reset }

  const timer = setInterval(() => {
    const now = Date.now();
    for (const [k, v] of hits) {
      if (v.reset <= now) hits.delete(k);
    }
  }, windowMs);
  timer.unref();

  return {
    // Records one hit for `key` and reports whether it is within the limit.
    check(key) {
      const now = Date.now();
      let rec = hits.get(key);
      if (!rec || rec.reset <= now) {
        rec = { count: 0, reset: now + windowMs };
        hits.set(key, rec);
      }
      rec.count += 1;
      const allowed = rec.count <= max;
      const retryAfterSec = Math.max(1, Math.ceil((rec.reset - now) / 1000));
      return { allowed, retryAfterSec };
    },
  };
}

// Resolve the client IP. Only trust X-Forwarded-For when explicitly enabled,
// since clients can otherwise spoof it to evade per-IP limits.
export function clientIp(req, trustProxy) {
  if (trustProxy) {
    const xff = req.headers["x-forwarded-for"];
    if (xff) {
      const first = String(xff).split(",")[0].trim();
      if (first) return first;
    }
  }
  return req.socket?.remoteAddress || "unknown";
}
