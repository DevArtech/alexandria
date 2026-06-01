#!/usr/bin/env bash
# Point git at the tracked hooks directory so the pre-commit gate runs locally.
set -euo pipefail

repo_root="$(git rev-parse --show-toplevel)"
cd "$repo_root"

git config core.hooksPath .githooks
chmod +x .githooks/* 2>/dev/null || true

echo "✓ Git hooks enabled (core.hooksPath=.githooks)."
echo "  The pre-commit gate runs rustfmt/clippy/tests and prettier/eslint/typecheck."
