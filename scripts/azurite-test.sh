#!/usr/bin/env bash
# Run the full test suite, including the Azure store tests against Azurite (which need the
# `azurite` feature: it adds the `sas` storage auth mode they use).
#
# Uses an Azurite already listening on 127.0.0.1:10000 if there is one; otherwise starts one
# with `npx azurite-blob` (in-memory, stopped on exit).
set -euo pipefail

# Account SAS signed with Azurite's PUBLIC well-known dev key (not a secret).
export AZURITE_SAS_TOKEN='se=2035-01-01T00%3A00Z&sp=rwdlac&sv=2026-04-06&ss=b&srt=sco&sig=cO88Suxa2pemR0IUIqL5uQm6F%2BL1Z18nhX7zSLZ8up8%3D'
# Read-only (sp=rl) SAS for the nx-cache-test container, same key: a store that may read but not write.
export AZURITE_READONLY_SAS_TOKEN='se=2035-01-01T00%3A00Z&sp=rl&sv=2026-04-06&sr=c&sig=csmnGXB3cU%2BdKTItElaTykucnn9UtZslP65aLYjq3J8%3D'
export AZURITE_ENDPOINT="${AZURITE_ENDPOINT:-http://127.0.0.1:10000/devstoreaccount1}"
export AZURITE_CONTAINER="${AZURITE_CONTAINER:-nx-cache-test}"

create_container() {
  curl -s -o /dev/null -w '%{http_code}' -X PUT -H 'x-ms-version: 2025-01-05' -H 'Content-Length: 0' \
    "$AZURITE_ENDPOINT/$AZURITE_CONTAINER?restype=container&$AZURITE_SAS_TOKEN" || true
}

if ! curl -s -o /dev/null "$AZURITE_ENDPOINT"; then
  echo "starting Azurite..."
  # Own process group (job control on), so the whole npx → node tree can be stopped together.
  # Output goes to a file so the emulator never holds this script's stdout open.
  set -m
  npx --yes --package=azurite@3 azurite-blob --blobHost 127.0.0.1 --blobPort 10000 \
    --inMemoryPersistence --skipApiVersionCheck --loose > "${TMPDIR:-/tmp}/azurite-test.log" 2>&1 &
  AZURITE_PGID=$!
  set +m
  trap 'kill -- -$AZURITE_PGID 2>/dev/null || true' EXIT
fi

for _ in $(seq 1 60); do
  code=$(create_container)
  if [ "$code" = 201 ] || [ "$code" = 409 ]; then break; fi
  sleep 1
done
if [ "$code" != 201 ] && [ "$code" != 409 ]; then
  echo "could not create container $AZURITE_CONTAINER (HTTP $code)" >&2
  exit 1
fi

cargo test --features azurite "$@"
