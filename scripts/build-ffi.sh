#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

cargo build -p aether-ffi

HEADER="$ROOT/macos/AetherFFI/include/aether_ffi.h"
if command -v cbindgen >/dev/null; then
  cbindgen --crate aether-ffi -c crates/aether-ffi/cbindgen.toml -o "$HEADER"
else
  echo "cbindgen not found — using checked-in $HEADER"
fi

echo "FFI staticlib: $ROOT/target/debug/libaether_ffi.a"
