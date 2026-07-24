#!/usr/bin/env bash
# Notarize a signed DMG with Apple notarytool (Phase 5).
# Requires: Apple Developer ID Application cert, notarytool credentials.
set -euo pipefail

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "notarize.sh requires macOS" >&2
  exit 1
fi

DMG="${1:-}"
if [[ -z "$DMG" || ! -f "$DMG" ]]; then
  echo "Usage: $0 <path-to-signed.dmg>" >&2
  echo "" >&2
  echo "Prerequisites:" >&2
  echo "  1. Apple Developer Program membership" >&2
  echo "  2. Developer ID Application certificate in Keychain" >&2
  echo "  3. xcrun notarytool store-credentials (or NOTARY_APPLE_ID + team)" >&2
  exit 1
fi

SIGN_ID="${CODESIGN_IDENTITY:-Developer ID Application}"
PROFILE="${NOTARY_PROFILE:-AetherForge-notary}"

if ! security find-identity -v -p codesigning | grep -q "$SIGN_ID"; then
  echo "No codesigning identity matching '$SIGN_ID'." >&2
  echo "Set CODESIGN_IDENTITY or install a Developer ID Application certificate." >&2
  exit 1
fi

echo "==> Submitting $DMG for notarization (profile: $PROFILE)"
xcrun notarytool submit "$DMG" --keychain-profile "$PROFILE" --wait

echo "==> Stapling ticket"
xcrun stapler staple "$DMG"

echo "Notarization complete: $DMG"
