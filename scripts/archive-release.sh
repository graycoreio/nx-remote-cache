#!/usr/bin/env bash
# Packs what scripts/build-release.sh built into the release archive:
#   scripts/archive-release.sh <out-dir> <target-name>
# → dist/nx-remote-cache-<version>-<target-name>.tar.gz
# <version> is the tag without its `v`, or `dev` outside a tag run.
set -euo pipefail

out=$1
name=$2
if [ "${GITHUB_REF_TYPE:-}" = tag ]; then version=${GITHUB_REF_NAME#v}; else version=dev; fi

archive="dist/nx-remote-cache-$version-$name.tar.gz"
mkdir -p dist
cp LICENSE "$out/"
tar -czf "$archive" -C "$out" nx-remote-cache LICENSE
echo "$archive"
