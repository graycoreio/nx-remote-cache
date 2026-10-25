#!/usr/bin/env bash
# Builds the release binary for one platform:
#   scripts/build-release.sh <out-dir> <target>...
# Writes <out-dir>/nx-remote-cache. Given several targets (macOS: aarch64 + x86_64), it is one
# universal binary.
# CI's release-build job runs the same `cargo build`, so the cache it saves on main is the one
# release builds start from.
set -euo pipefail

out=$1
shift

binaries=()
for target in "$@"; do
  cargo build --release --locked --target "$target"
  binaries+=("target/$target/release/nx-remote-cache")
done

mkdir -p "$out"
if [ "${#binaries[@]}" -gt 1 ]; then
  lipo -create -output "$out/nx-remote-cache" "${binaries[@]}"
  lipo -info "$out/nx-remote-cache"
else
  cp "${binaries[0]}" "$out/nx-remote-cache"
fi
