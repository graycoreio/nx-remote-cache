#!/usr/bin/env bash
# Packs what packages/server/scripts/build-release.sh built into the release archive.
#
# Usage:
#   packages/server/scripts/archive-release.sh <out-dir> <target-name>
#
# Arguments:
#   <out-dir>      The folder build-release.sh wrote the nx-remote-cache binary to.
#   <target-name>  The platform, used in the archive name: a Rust target such as
#                  x86_64-unknown-linux-gnu, or universal-apple-darwin.
#
# Environment:
#   GITHUB_REF_TYPE, GITHUB_REF_NAME  On a tag run, the version is the tag without its `v`.
#   CI                                Set (to anything non-empty) by CI systems, including
#                                     GitHub Actions. If set and GITHUB_REF_TYPE isn't `tag`,
#                                     the version is `dev`; if unset, as on a developer's machine,
#                                     it is `local`.
#
# Output:
#   dist/nx-remote-cache-<version>-<target-name>.tar.gz, holding the binary and LICENSE.
#
# Example:
#   packages/server/scripts/build-release.sh target/release-out x86_64-unknown-linux-gnu
#   packages/server/scripts/archive-release.sh target/release-out x86_64-unknown-linux-gnu
set -euo pipefail

out=$1
name=$2
if [ "${GITHUB_REF_TYPE:-}" = tag ]; then
  version=${GITHUB_REF_NAME#v}
elif [ -n "${CI:-}" ]; then
  version=dev
else
  version=local
fi

archive="dist/nx-remote-cache-$version-$name.tar.gz"
mkdir -p dist
cp LICENSE "$out/"
tar -czf "$archive" -C "$out" nx-remote-cache LICENSE
echo "$archive"
