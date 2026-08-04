#!/usr/bin/env bash
# Sign an AetherForge .app bundle with Developer ID (inside-out, Hardened Runtime).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
# shellcheck source=scripts/lib/distribution.sh
source "$ROOT/scripts/lib/distribution.sh"

distribution_require_darwin "sign-app.sh"

APP_BUNDLE="${1:-}"
if [[ -z "$APP_BUNDLE" || ! -d "$APP_BUNDLE" ]]; then
  echo "Usage: $0 <path-to/AetherForge.app>" >&2
  echo "" >&2
  echo "Environment:" >&2
  echo "  CODESIGN_IDENTITY   Developer ID Application identity (auto-detected if unset)" >&2
  exit 1
fi

IDENTITY="$(distribution_resolve_sign_identity)"
if [[ -z "$IDENTITY" ]]; then
  echo "No Developer ID Application certificate found in Keychain." >&2
  echo "Install one from Apple Developer Program, or set CODESIGN_IDENTITY." >&2
  exit 1
fi

echo "Using signing identity: $IDENTITY"
distribution_sign_app_inside_out "$APP_BUNDLE" "$IDENTITY"
echo "Signed: $APP_BUNDLE"
