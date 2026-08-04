#!/usr/bin/env bash
# Notarize signed AetherForge artifacts with Apple notarytool (Phase 8.12 / Track D.2).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
# shellcheck source=scripts/lib/distribution.sh
source "$ROOT/scripts/lib/distribution.sh"

distribution_require_darwin "notarize.sh"

usage() {
  cat <<EOF
Usage: $0 <path-to.dmg> [path-to/AetherForge.app]

Submit a signed DMG to Apple notarytool, then staple both the DMG and (optionally) the .app.

Prerequisites (local maintainer machine):
  1. Apple Developer Program membership
  2. Developer ID Application certificate in Keychain
  3. notarytool credentials stored once:
       xcrun notarytool store-credentials AetherForge-notary \\
         --apple-id "you@example.com" --team-id TEAMID --password "@keychain:AC_PASSWORD"

Environment:
  NOTARY_PROFILE          Keychain profile name (default: AetherForge-notary)
  CODESIGN_IDENTITY       Used only for preflight cert check
  AETHER_SKIP_NOTARIZE=1  Skip when creds/certs unavailable (CI unsigned path)

CI: leave AETHER_SKIP_NOTARIZE unset and omit Apple secrets — script exits 0 with SKIP
    when neither notary profile nor signing identity is configured.
EOF
}

DMG="${1:-}"
APP_BUNDLE="${2:-}"

if [[ -z "$DMG" || ! -f "$DMG" ]]; then
  usage >&2
  exit 1
fi

if [[ "${AETHER_SKIP_NOTARIZE:-0}" == "1" ]]; then
  echo "Skipping notarization (AETHER_SKIP_NOTARIZE=1)."
  exit 0
fi

PROFILE="${NOTARY_PROFILE:-AetherForge-notary}"
SIGN_ID="$(distribution_resolve_sign_identity)"

if ! distribution_has_sign_identity; then
  echo "SKIP: No Developer ID Application certificate — notarization requires signing first." >&2
  echo "      Build with AETHER_SIGN=1 ./scripts/create-dmg.sh on a machine with your cert." >&2
  exit 0
fi

if ! distribution_notary_profile_ready; then
  echo "SKIP: notarytool profile '$PROFILE' not found in Keychain." >&2
  echo "      Run: xcrun notarytool store-credentials $PROFILE ..." >&2
  exit 0
fi

if [[ -n "$APP_BUNDLE" && ! -d "$APP_BUNDLE" ]]; then
  echo "App bundle not found: $APP_BUNDLE" >&2
  exit 1
fi

echo "==> Preflight: verifying signed app inside DMG"
AETHER_REQUIRE_SIGNED=1 "$ROOT/scripts/verify-codesign.sh" "$DMG"

echo "==> Submitting DMG for notarization (profile: $PROFILE)"
xcrun notarytool submit "$DMG" --keychain-profile "$PROFILE" --wait

echo "==> Stapling notarization ticket to DMG"
xcrun stapler staple "$DMG"
xcrun stapler validate "$DMG"

if [[ -n "$APP_BUNDLE" ]]; then
  echo "==> Stapling notarization ticket to .app"
  xcrun stapler staple "$APP_BUNDLE"
  xcrun stapler validate "$APP_BUNDLE"
fi

echo ""
echo "Notarization complete."
echo "  DMG: $DMG"
[[ -n "$APP_BUNDLE" ]] && echo "  App: $APP_BUNDLE"
echo "Verify: AETHER_REQUIRE_NOTARIZED=1 ./scripts/verify-codesign.sh \"$DMG\""
