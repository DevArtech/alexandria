// Minimal leveled logger writing structured-ish lines to stderr, so it never
// interferes with anything a transport might expect on stdout.
const LEVELS = { error: 0, warn: 1, info: 2, debug: 3 };

export function createLogger(level = "info") {
  const threshold = LEVELS[level] ?? LEVELS.info;
  const emit = (lvl) => (msg, extra) => {
    if (LEVELS[lvl] > threshold) return;
    const line = `${new Date().toISOString()} ${lvl.toUpperCase().padEnd(5)} ${msg}`;
    process.stderr.write(extra ? `${line} ${JSON.stringify(extra)}\n` : `${line}\n`);
  };
  return {
    error: emit("error"),
    warn: emit("warn"),
    info: emit("info"),
    debug: emit("debug"),
  };
}
