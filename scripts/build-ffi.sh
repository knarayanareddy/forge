#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

PROFILE="${1:-debug}"
if [[ "$PROFILE" == "release" ]]; then
  cargo build --release -p aether-ffi
  LIB="$ROOT/target/release/libaether_ffi.a"
else
  cargo build -p aether-ffi
  LIB="$ROOT/target/debug/libaether_ffi.a"
fi

HEADER="$ROOT/macos/AetherFFI/include/aether_ffi.h"
if command -v cbindgen >/dev/null; then
  cbindgen --crate aether-ffi -c crates/aether-ffi/cbindgen.toml -o "$HEADER"
else
  echo "cbindgen not found — using checked-in $HEADER"
fi

echo "FFI staticlib: $LIB"
