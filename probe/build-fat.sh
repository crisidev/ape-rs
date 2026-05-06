#!/usr/bin/env bash
# Build a fat (x86_64 + aarch64) APE from the probe crate.
#
# Requires:
#   - cosmocc 4.x release tree (set COSMO to its top level)
#   - nightly rustc with rust-src component
#
# Usage:
#   COSMO=/path/to/cosmocc-4.0.2 ./build-fat.sh [--release]
set -euo pipefail

: "${COSMO:?set COSMO to the cosmocc release tree (e.g. .../cosmocc-4.0.2)}"

PROFILE_DIR="debug"
CARGO_EXTRA=()
while [[ $# -gt 0 ]]; do
  case "$1" in
    --release) PROFILE_DIR="release"; CARGO_EXTRA+=(--release); shift;;
    *) echo "unknown arg: $1" >&2; exit 2;;
  esac
done

export PATH="$COSMO/bin:$PATH"

ARCH=x86_64 cargo +nightly build "${CARGO_EXTRA[@]}" \
  --target=./x86_64-unknown-linux-cosmo.json -Z json-target-spec

ARCH=aarch64 cargo +nightly build "${CARGO_EXTRA[@]}" \
  --target=./aarch64-unknown-linux-cosmo.json -Z json-target-spec

"$COSMO/bin/apelink" \
  -l "$COSMO/bin/ape-x86_64.elf" \
  -l "$COSMO/bin/ape-aarch64.elf" \
  -M "$COSMO/bin/ape-m1.c" \
  -o ./probe.com \
  "./target/x86_64-unknown-linux-cosmo/${PROFILE_DIR}/probe.com.dbg" \
  "./target/aarch64-unknown-linux-cosmo/${PROFILE_DIR}/probe.com.dbg"

ls -la ./probe.com
file ./probe.com
