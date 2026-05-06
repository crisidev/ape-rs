#!/usr/bin/env bash
# Apply the Strategy-2 prototype patches to a rustup nightly's std
# sources so it compiles against our cfg(cosmo)-patched rust-libc fork.
#
# Usage:
#   ./apply.sh [STD_ROOT] [LIBC_COSMO_PATH]
#
# Defaults:
#   STD_ROOT         = active nightly's library/
#   LIBC_COSMO_PATH  = ../libc-cosmo/  (absolute, resolved from this script)
#
# Each patch file in patches/std/ is applied with `patch -d $STD_ROOT
# -p1`. The Cargo.toml patch contains a `@LIBC_COSMO_PATH@` placeholder
# for the libc dep path; we substitute it at apply time so the patch
# file itself stays portable.
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
  echo "  expected to find $STD_ROOT/std/" >&2
  echo "  usage: $0 [STD_ROOT] [LIBC_COSMO_PATH]" >&2
  exit 1
fi
if [[ ! -d "$LIBC_COSMO_PATH" ]]; then
  echo "ERROR: $LIBC_COSMO_PATH is not a directory" >&2
  echo "  pass the absolute path to the libc-cosmo/ submodule" >&2
  exit 1
fi

echo "STD_ROOT=        $STD_ROOT"
echo "LIBC_COSMO_PATH= $LIBC_COSMO_PATH"
echo

# Sanity: the patches are built against the nightly version our sysroot
# patches were developed on. Other nightlies may drift; patches may fail
# to apply cleanly on different line numbers. That's a signal to read
# README.md's per-patch walkthrough and hand-reapply.
apply_patch() {
  local patch="$1"
  local name="$(basename "$patch")"
  if [[ "$name" == "Cargo.patch" ]]; then
    # Substitute the libc path placeholder.
    sed "s|@LIBC_COSMO_PATH@|$LIBC_COSMO_PATH|g" "$patch" \
      | patch -d "$STD_ROOT" -p2 --forward --silent
  else
    patch -d "$STD_ROOT" -p2 --forward --silent < "$patch"
  fi
  echo "applied: $name"
}

for p in "$HERE/std/"*.patch; do
  apply_patch "$p"
done

echo
echo "done — $STD_ROOT patched. run ./revert.sh to undo."
