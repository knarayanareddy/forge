#!/usr/bin/env bash
# Build release artifacts and create a drag-and-drop DMG for AetherForge (Phase 5).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

APP_NAME="AetherForge"
VERSION="${AETHER_VERSION:-0.1.0}"
BUILD_DIR="$ROOT/build/dmg"
STAGING="$BUILD_DIR/staging"
APP_BUNDLE="$STAGING/${APP_NAME}.app"
DMG_PATH="$BUILD_DIR/${APP_NAME}-${VERSION}.dmg"

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "create-dmg.sh requires macOS" >&2
  exit 1
fi

echo "==> Building Rust release (daemon + FFI)"
cargo build --release -p aether-daemon -p aether-ffi
./scripts/build-ffi.sh release

echo "==> Building Swift app (release)"
swift build -c release

echo "==> Staging .app bundle"
rm -rf "$STAGING"
mkdir -p "$APP_BUNDLE/Contents/MacOS" "$APP_BUNDLE/Contents/Resources"
mkdir -p "$APP_BUNDLE/Contents/Resources/profiles"

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

echo "==> Creating DMG"
rm -f "$DMG_PATH"
hdiutil create -volname "$APP_NAME" -srcfolder "$STAGING" -ov -format UDZO "$DMG_PATH"

echo "DMG ready: $DMG_PATH"
echo "Optional: sign with codesign, then run ./scripts/notarize.sh $DMG_PATH"
