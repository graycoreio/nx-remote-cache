#!/usr/bin/env bash
# Print a short-lived user-delegation SAS for one container, for running the server in Docker
# on a dev machine (`auth = { mode = "sas" }`). Signed with your own `az login` identity, so
# no account keys are involved. You need a data role on the container (Contributor to write,
# Reader to read; the SAS never allows more) and Storage Blob Delegator on the account, which
# deploy/local.bicep grants its developer groups. Without it: AuthorizationPermissionMismatch.
#
#   export AZURE_STORAGE_SAS_TOKEN=$(scripts/dev-sas.sh <account> <container> [days, max 7])
set -euo pipefail

account="${1:?usage: dev-sas.sh <account> <container> [days]}"
container="${2:?usage: dev-sas.sh <account> <container> [days]}"
days="${3:-1}"
expiry=$(date -u -d "+${days} days" '+%Y-%m-%dT%H:%MZ' 2>/dev/null || date -u -v+"${days}"d '+%Y-%m-%dT%H:%MZ')

az storage container generate-sas \
  --account-name "$account" \
  --name "$container" \
  --as-user --auth-mode login \
  --permissions racw \
  --expiry "$expiry" \
  --https-only \
  --output tsv
