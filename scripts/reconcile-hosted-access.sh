#!/usr/bin/env bash
# Revokes access that a hosted.bicep deployment no longer lists. Azure deployments only add and
# update: a subject or developer group removed from the parameters keeps its federated
# credential or app role grant until something deletes it. This script compares what exists
# with the parameters of the resource group's last hosted.bicep deployment and deletes the rest.
#
#   scripts/reconcile-hosted-access.sh <resource group> [deployment name, default: hosted]           # dry run
#   scripts/reconcile-hosted-access.sh <resource group> [deployment name, default: hosted] --apply   # delete
#
# Run it after every hosted.bicep deployment that removes subjects or groups. It touches only the
# deployment's own CI identities and its API's app role grants to groups; grants it didn't make
# (other principals) are reported, never deleted. local.bicep deployments don't need it: deploy
# them as a stack (README, "Local instances"), which deletes what the template no longer lists.
set -euo pipefail

rg="${1:?usage: reconcile-hosted-access.sh <resource group> [deployment name] [--apply]}"
deployment=hosted
apply=false
for arg in "${@:2}"; do
  case "$arg" in
    --apply) apply=true ;;
    *) deployment="$arg" ;;
  esac
done

query() { az deployment group show -g "$rg" -n "$deployment" --query "$1" -o tsv; }
prefix=$(query properties.parameters.namePrefix.value)
[ -n "$prefix" ] || { echo "deployment '$deployment' in $rg has no namePrefix parameter; is it a hosted.bicep deployment?" >&2; exit 1; }
api_client_id=$(query properties.outputs.apiClientId.value)
writer_subjects=$(query "properties.parameters.writerSubjects.value[]")
reader_subjects=$(query "properties.parameters.readerSubjects.value[]")
reader_groups=$(query "properties.parameters.readerGroupObjectIds.value[]")
writer_groups=$(query "properties.parameters.writerGroupObjectIds.value[]")

listed() { [ -n "$2" ] && grep -Fxq -- "$1" <<<"$2"; }
removed=0
remove() {
  removed=$((removed + 1))
  if $apply; then echo "delete  $1"; "${@:2}"; else echo "would delete  $1"; fi
}

echo "== Federated credentials (deployment '$deployment', prefix '$prefix')"
for role in writer reader; do
  identity="$prefix-ci-$role"
  allowed=$([ $role = writer ] && echo "$writer_subjects" || echo "$reader_subjects")
  while IFS=$'\t' read -r name subject; do
    [ -n "$name" ] || continue
    if listed "$subject" "$allowed"; then
      echo "keep    $identity: $subject"
    else
      remove "$identity: $subject" az identity federated-credential delete -g "$rg" --identity-name "$identity" -n "$name" --yes
    fi
  done < <(az identity federated-credential list -g "$rg" --identity-name "$identity" --query "[].[name, subject]" -o tsv)
done

echo "== App role grants on the cache API ($api_client_id)"
sp=$(az ad sp show --id "$api_client_id" --query id -o tsv)
read_role=$(az ad sp show --id "$api_client_id" --query "appRoles[?value=='Cache.Read'].id | [0]" -o tsv)
write_role=$(az ad sp show --id "$api_client_id" --query "appRoles[?value=='Cache.Write'].id | [0]" -o tsv)
grants=$(az rest --method GET --url "https://graph.microsoft.com/v1.0/servicePrincipals/$sp/appRoleAssignedTo" \
  --query "value[].[id, principalType, principalId, appRoleId, principalDisplayName]" -o tsv)
while IFS=$'\t' read -r id type principal role display; do
  [ -n "$id" ] || continue
  case "$role" in
    "$read_role") role_name=Cache.Read ;;
    "$write_role") role_name=Cache.Write ;;
    *) role_name=$role ;;
  esac
  if [ "$type" != Group ]; then
    # The CI identities' grants, and anything added by hand: not this script's business.
    echo "keep    $type $display ($principal): $role_name"
  elif { [ "$role" = "$read_role" ] && listed "$principal" "$reader_groups"; } ||
       { [ "$role" = "$write_role" ] && listed "$principal" "$writer_groups"; }; then
    echo "keep    group $display ($principal): $role_name"
  else
    remove "group $display ($principal): $role_name" \
      az rest --method DELETE --url "https://graph.microsoft.com/v1.0/servicePrincipals/$sp/appRoleAssignedTo/$id"
  fi
done <<<"$grants"

if [ "$removed" -eq 0 ]; then
  echo "Nothing to revoke."
elif ! $apply; then
  echo "Dry run: $removed to revoke. Run again with --apply to delete them."
fi
