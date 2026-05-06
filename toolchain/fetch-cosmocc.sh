#!/usr/bin/env bash
# Download and unpack cosmocc 4.0.2 into ./toolchain/cosmocc-4.0.2/.
#
# After running this, point the per-workload build scripts at it via:
#   COSMO=$(pwd)/toolchain/cosmocc-4.0.2 ./build-fat.sh --release
set -euo pipefail

VERSION="4.0.2"
URL="https://github.com/jart/cosmopolitan/releases/download/${VERSION}/cosmocc-${VERSION}.zip"

HERE="$(cd "$(dirname "$0")" && pwd)"
DEST="$HERE/cosmocc-${VERSION}"
ZIP="$HERE/cosmocc-${VERSION}.zip"

if [[ -d "$DEST/bin" && -x "$DEST/bin/apelink" ]]; then
  echo "cosmocc ${VERSION} already present at $DEST"
  exit 0
fi

mkdir -p "$DEST"

if [[ ! -f "$ZIP" ]]; then
  echo "downloading $URL"
  if command -v curl >/dev/null 2>&1; then
    curl -fL --retry 3 -o "$ZIP" "$URL"
  elif command -v wget >/dev/null 2>&1; then
    wget -O "$ZIP" "$URL"
  else
    echo "ERROR: need curl or wget" >&2
    exit 1
  fi
fi

echo "extracting into $DEST"
unzip -q -o "$ZIP" -d "$DEST"

# Sanity check.
if [[ ! -x "$DEST/bin/apelink" ]]; then
  echo "ERROR: $DEST/bin/apelink missing after extract" >&2
  exit 1
fi

echo "done: $DEST"
echo
echo "Next:"
echo "  export COSMO=$DEST"
echo "  cd rust-ape-example && \$COSMO/../../patches/apply.sh && ./build-fat.sh --release"
