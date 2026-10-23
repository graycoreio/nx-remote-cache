#!/usr/bin/env bash
# Checks that a built server binary runs and serves: scripts/smoke-test-binary.sh <path>
# Used by the release workflow on each platform it builds for.
set -euo pipefail

bin=$1
port=${SMOKE_PORT:-3999}

"$bin" hash-token smoke >/dev/null

NX_CACHE_CONFIG_TOML="$(printf '[server]\nlisten = "127.0.0.1:%s"\n[storage]\ntype = "memory"\n[[auth.providers]]\ntype = "anonymous"\nscope = "read"\n' "$port")" \
  "$bin" serve &
pid=$!
trap 'kill "$pid" 2>/dev/null || true' EXIT

for _ in $(seq 1 20); do
  curl -sf "http://127.0.0.1:$port/health" >/dev/null && break
  sleep 1
done
status() { curl -s -o /dev/null -w '%{http_code}' "$@"; }
test "$(status "http://127.0.0.1:$port/v1/cache/123")" = 404
test "$(status -X PUT -H 'Content-Length: 1' --data x "http://127.0.0.1:$port/v1/cache/123")" = 403
echo "ok: $bin"
