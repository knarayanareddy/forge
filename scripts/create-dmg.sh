#!/usr/bin/env bash
# Build release artifacts and create a drag-and-drop DMG for AetherForge (Phase 8.12 / Track D).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
# shellcheck source=scripts/lib/distribution.sh
source "$ROOT/scripts/lib/distribution.sh"

APP_NAME="AetherForge"
VERSION="${AETHER_VERSION:-0.1.0}"
BUILD_DIR="$ROOT/build/dmg"
STAGING="$BUILD_DIR/staging"
APP_BUNDLE="$STAGING/${APP_NAME}.app"
DMG_PATH="$BUILD_DIR/${APP_NAME}-${VERSION}.dmg"
SKIP_BUILD="${AETHER_SKIP_BUILD:-0}"

usage() {
  cat <<EOF
Usage: $0 [options]

Build AetherForge.app, optionally sign with Developer ID, and create a DMG.

Options:
  --sign              Sign .app when a Developer ID cert is available (same as AETHER_SIGN=1)
  --skip-build        Reuse existing release binaries (AETHER_SKIP_BUILD=1)
  -h, --help          Show this help

Environment:
  AETHER_VERSION          Bundle/DMG version (default: 0.1.0)
  AETHER_SIGN=1           Sign when identity is present; warn and continue unsigned if missing
  AETHER_SKIP_BUILD=1     Skip cargo/swift rebuild
  CODESIGN_IDENTITY       Explicit Developer ID Application identity

Outputs:
  $APP_BUNDLE
  $DMG_PATH

Next steps (Developer ID required):
  ./scripts/notarize.sh "$DMG_PATH" "$APP_BUNDLE"
  ./scripts/verify-codesign.sh "$DMG_PATH"
EOF
}

SIGN_REQUESTED=0
while [[ $# -gt 0 ]]; do
  case "$1" in
    --sign) SIGN_REQUESTED=1; shift ;;
    --skip-build) SKIP_BUILD=1; shift ;;
    -h|--help) usage; exit 0 ;;
    *) echo "Unknown option: $1" >&2; usage >&2; exit 1 ;;
  esac
done

if distribution_should_sign || [[ "$SIGN_REQUESTED" == "1" ]]; then
  export AETHER_SIGN=1
fi

distribution_require_darwin "create-dmg.sh"

if [[ "$SKIP_BUILD" != "1" ]]; then
  echo "==> Building Rust release (daemon + FFI)"
  cargo build --release -p aether-daemon -p aether-ffi
  ./scripts/build-ffi.sh release

  echo "==> Building Swift app (release)"
  swift build -c release
else
  echo "==> Skipping build (AETHER_SKIP_BUILD=1)"
fi

echo "==> Staging .app bundle"
rm -rf "$STAGING"
mkdir -p "$APP_BUNDLE/Contents/MacOS" "$APP_BUNDLE/Contents/Resources/profiles"

BIN="$ROOT/.build/release/AetherForgeApp"
DAEMON="$ROOT/target/release/aether-daemon"

if [[ ! -f "$BIN" ]]; then
  echo "Missing Swift binary: $BIN" >&2
  exit 1
fi
if [[ ! -f "$DAEMON" ]]; then
  echo "Missing daemon binary: $DAEMON" >&2
  exit 1
fi

cp "$BIN" "$APP_BUNDLE/Contents/MacOS/${APP_NAME}"
cp "$DAEMON" "$APP_BUNDLE/Contents/MacOS/aether-daemon"
cp "$ROOT/profiles/sandbox_tool.sb" "$APP_BUNDLE/Contents/Resources/profiles/sandbox_tool.sb"
chmod +x "$APP_BUNDLE/Contents/MacOS/"*

cat > "$APP_BUNDLE/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleExecutable</key><string>${APP_NAME}</string>
  <key>CFBundleIdentifier</key><string>dev.aetherforge.app</string>
  <key>CFBundleName</key><string>${APP_NAME}</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>${VERSION}</string>
  <key>CFBundleVersion</key><string>${VERSION}</string>
  <key>LSMinimumSystemVersion</key><string>15.0</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST

if distribution_should_sign; then
  if distribution_has_sign_identity; then
    IDENTITY="$(distribution_resolve_sign_identity)"
    echo "==> Signing staged app (AETHER_SIGN=1)"
    distribution_sign_app_inside_out "$APP_BUNDLE" "$IDENTITY"
  else
    echo "WARN: AETHER_SIGN=1 but no Developer ID Application cert found; continuing unsigned." >&2
    echo "      Install a cert or unset AETHER_SIGN for ad-hoc local DMGs." >&2
  fi
fi

echo "==> Creating DMG"
rm -f "$DMG_PATH"
hdiutil create -volname "$APP_NAME" -srcfolder "$STAGING" -ov -format UDZO "$DMG_PATH"

SHA256="$(shasum -a 256 "$DMG_PATH" | awk '{print $1}')"
echo "$SHA256  $(basename "$DMG_PATH")" > "${DMG_PATH}.sha256"

echo ""
echo "DMG ready: $DMG_PATH"
echo "SHA256:    $SHA256"
echo "App bundle: $APP_BUNDLE"
if distribution_should_sign && distribution_has_sign_identity; then
  echo "Signed with: $(distribution_resolve_sign_identity)"
  echo "Next: ./scripts/notarize.sh \"$DMG_PATH\" \"$APP_BUNDLE\""
else
  echo "Unsigned DMG (valid for local/testing)."
  echo "To sign: AETHER_SIGN=1 $0 --skip-build"
  echo "Verify:  ./scripts/verify-codesign.sh \"$DMG_PATH\""
fi
echo "Homebrew: fill formulas/aetherforge.rb.template with version, sha256, and release URL."
