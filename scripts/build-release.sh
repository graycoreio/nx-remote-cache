#!/usr/bin/env bash
# Builds the release binaries, default and mint variants, for one platform:
#   scripts/build-release.sh <out-dir> <target>...
# Writes <out-dir>/default/nx-remote-cache and <out-dir>/mint/nx-remote-cache. Given several
# targets (macOS: aarch64 + x86_64), each variant becomes one universal binary.
# CI's release-build job runs the same `cargo build` for the default variant, so the cache it
# saves on main is the one release builds start from.
set -euo pipefail

out=$1
shift

for variant in default mint; do
  features=""
  [ "$variant" = mint ] && features="--features=mint"
  binaries=()
  for target in "$@"; do
    # $features is deliberately unquoted: empty means no argument.
    cargo build --release --locked --target "$target" $features
    binaries+=("target/$target/release/nx-remote-cache")
  done

  mkdir -p "$out/$variant"
  if [ "${#binaries[@]}" -gt 1 ]; then
    lipo -create -output "$out/$variant/nx-remote-cache" "${binaries[@]}"
    lipo -info "$out/$variant/nx-remote-cache"
  else
    cp "${binaries[0]}" "$out/$variant/nx-remote-cache"
  fi
done
