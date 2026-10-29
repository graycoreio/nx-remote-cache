#!/usr/bin/env bash
# Builds and archives the release binary for this machine's platform:
#   packages/server/scripts/archive-host.sh
# → dist/nx-remote-cache-<version>-<target-name>.tar.gz, as archive-release.sh names it.
set -euo pipefail

case "$(uname -s)-$(uname -m)" in
  Linux-x86_64) targets=(x86_64-unknown-linux-gnu); name=x86_64-unknown-linux-gnu ;;
  Linux-aarch64) targets=(aarch64-unknown-linux-gnu); name=aarch64-unknown-linux-gnu ;;
  # One universal binary, as releases build it.
  Darwin-*) targets=(aarch64-apple-darwin x86_64-apple-darwin); name=universal-apple-darwin ;;
  *) echo "no release build for $(uname -s)-$(uname -m)" >&2; exit 1 ;;
esac

packages/server/scripts/build-release.sh target/release-out "${targets[@]}"
packages/server/scripts/archive-release.sh target/release-out "$name"
