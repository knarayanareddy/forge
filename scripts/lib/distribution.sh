#!/usr/bin/env bash
# Shared helpers for AetherForge macOS distribution (Track D / Phase 8.12).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
ENTITLEMENTS="${ROOT}/packaging/entitlements/AetherForge.entitlements"

distribution_require_darwin() {
  if [[ "$(uname -s)" != "Darwin" ]]; then
    echo "${1:-distribution} requires macOS" >&2
    exit 1
  fi
}

distribution_resolve_sign_identity() {
  local identity="${CODESIGN_IDENTITY:-}"
  if [[ -z "$identity" ]]; then
    identity="$(security find-identity -v -p codesigning 2>/dev/null \
      | sed -n 's/.*"\(Developer ID Application:.*\)".*/\1/p' \
      | head -1 || true)"
  fi
  printf '%s' "$identity"
}

distribution_has_sign_identity() {
  local identity
  identity="$(distribution_resolve_sign_identity)"
  [[ -n "$identity" ]]
}

distribution_should_sign() {
  [[ "${AETHER_SIGN:-0}" == "1" || "${AETHER_SIGN:-}" == "true" ]]
}

distribution_sign_app_inside_out() {
  local app_bundle="$1"
  local identity="$2"

  if [[ ! -d "$app_bundle" ]]; then
    echo "App bundle not found: $app_bundle" >&2
    return 1
  fi
  if [[ ! -f "$ENTITLEMENTS" ]]; then
    echo "Missing entitlements: $ENTITLEMENTS" >&2
    return 1
  fi

  local sign_args=(
    --force
    --options runtime
    --entitlements "$ENTITLEMENTS"
    --timestamp
    --sign "$identity"
  )

  local macos_dir="$app_bundle/Contents/MacOS"
  local main_exe="$macos_dir/$(basename "$app_bundle" .app)"
  local daemon="$macos_dir/aether-daemon"

  echo "==> Signing Mach-O binaries inside-out (never --deep)"
  if [[ -f "$daemon" ]]; then
    echo "    aether-daemon"
    codesign "${sign_args[@]}" "$daemon"
  fi
  if [[ -f "$main_exe" ]]; then
    echo "    $(basename "$main_exe")"
    codesign "${sign_args[@]}" "$main_exe"
  fi

  echo "==> Signing app bundle"
  codesign "${sign_args[@]}" "$app_bundle"

  echo "==> Verifying signature (--strict)"
  codesign --verify --strict --verbose=2 "$app_bundle"
}

distribution_notary_profile_ready() {
  local profile="${NOTARY_PROFILE:-AetherForge-notary}"
  xcrun notarytool history --keychain-profile "$profile" >/dev/null 2>&1
}
