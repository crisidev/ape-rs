#!/usr/bin/env bash
# Revert the Strategy-2 patches from a rustup nightly's std sources.
# Uses `patch -R`. The Cargo.toml patch needs the same placeholder
# substitution as apply.sh so the reverse hunk matches what's on disk.
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"

if [[ -n "${1-}" ]]; then
  STD_ROOT="$1"
else
  SYSROOT=$(rustc +nightly --print sysroot)
  STD_ROOT="$SYSROOT/lib/rustlib/src/rust/library"
fi

if [[ -n "${2-}" ]]; then
  LIBC_COSMO_PATH="$2"
else
  LIBC_COSMO_PATH="$(cd "$HERE/../libc-cosmo" && pwd)"
fi

if [[ ! -d "$STD_ROOT/std" ]]; then
  echo "ERROR: $STD_ROOT doesn't look like a rust library/ tree" >&2
  exit 1
fi

for p in "$HERE/std/"*.patch; do
  name="$(basename "$p")"
  if [[ "$name" == "Cargo.patch" ]]; then
    sed "s|@LIBC_COSMO_PATH@|$LIBC_COSMO_PATH|g" "$p" \
      | patch -d "$STD_ROOT" -p2 -R --forward --silent
  else
    patch -d "$STD_ROOT" -p2 -R --forward --silent < "$p"
  fi
  echo "reverted: $name"
done

echo
echo "done — $STD_ROOT restored."
