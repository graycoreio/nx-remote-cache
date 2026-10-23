#!/usr/bin/env bash
# Packs what scripts/build-release.sh built into the release archives:
#   scripts/archive-release.sh <out-dir> <target-name>
# → dist/default/nx-remote-cache-<version>-<target-name>.tar.gz and
#   dist/mint/nx-remote-cache-mint-<version>-<target-name>.tar.gz
# <version> is the tag without its `v`, or `dev` outside a tag run.
set -euo pipefail

out=$1
name=$2
if [ "${GITHUB_REF_TYPE:-}" = tag ]; then version=${GITHUB_REF_NAME#v}; else version=dev; fi

for variant in default mint; do
  prefix=nx-remote-cache
  [ "$variant" = mint ] && prefix=nx-remote-cache-mint
  archive="dist/$variant/$prefix-$version-$name.tar.gz"
  mkdir -p "dist/$variant"
  cp LICENSE "$out/$variant/"
  tar -czf "$archive" -C "$out/$variant" nx-remote-cache LICENSE
  echo "$archive"
done
