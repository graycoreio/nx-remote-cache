#!/usr/bin/env bash
# Runs docker-compose.dev.yml, first making sure the token sidecar's shared secret exists.
#
# Compose can't generate random values, so the first run writes a random
# NX_CACHE_IDENTITY_HEADER to .env (git-ignored), which Compose reads on every later run.
#
#   scripts/dev-compose.sh run --rm az az login --use-device-code   # once
#   scripts/dev-compose.sh                                          # = up -d
#   scripts/dev-compose.sh down
set -euo pipefail

cd "$(dirname "$0")/.."

if ! grep -qs '^NX_CACHE_IDENTITY_HEADER=' .env; then
  touch .env
  chmod 600 .env # before the secret is written
  echo "NX_CACHE_IDENTITY_HEADER=$(openssl rand -hex 32)" >> .env
  echo "generated NX_CACHE_IDENTITY_HEADER in .env" >&2
fi

if [ $# -eq 0 ]; then
  set -- up -d
fi
exec docker compose -f docker-compose.dev.yml "$@"
