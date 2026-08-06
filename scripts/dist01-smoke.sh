#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
source "$ROOT/scripts/lib/distribution.sh"
distribution_require_darwin "dist01-smoke.sh"
ARTIFACT="${1:-}"
TMP_STAGING=""
cleanup() { [[ -n "$TMP_STAGING" && -d "$TMP_STAGING" ]] && rm -rf "$TMP_STAGING"; }
trap cleanup EXIT
if [[ -z "$ARTIFACT" ]]; then
  TMP_STAGING="$(mktemp -d "${TMPDIR:-/tmp}/aetherforge-dist01.XXXXXX")"
  APP="$TMP_STAGING/Dist01Probe.app"
  mkdir -p "$APP/Contents/MacOS"
  printf '#!/bin/sh\nexit 0\n' > "$APP/Contents/MacOS/Dist01Probe"
  chmod +x "$APP/Contents/MacOS/Dist01Probe"
  cat > "$APP/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleExecutable</key><string>Dist01Probe</string>
  <key>CFBundleIdentifier</key><string>dev.aetherforge.dist01-probe</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>0.0.0</string>
  <key>CFBundleVersion</key><string>0</string>
</dict></plist>
PLIST
  ARTIFACT="$APP"
fi
chmod +x "$ROOT/scripts/verify-codesign.sh" 2>/dev/null || true
unset AETHER_REQUIRE_SIGNED AETHER_REQUIRE_NOTARIZED
"$ROOT/scripts/verify-codesign.sh" "$ARTIFACT"
if spctl --assess --type execute -v "$ARTIFACT" >/dev/null 2>&1; then
  echo "FAIL: spctl accepted unsigned probe" >&2; exit 1
fi
set +e
AETHER_REQUIRE_SIGNED=1 "$ROOT/scripts/verify-codesign.sh" "$ARTIFACT" >/dev/null 2>&1
[[ $? -eq 0 ]] && { echo "FAIL: REQUIRE_SIGNED passed unsigned" >&2; exit 1; }
set -e
grep -q sparkle:edSignature "$ROOT/packaging/sparkle/appcast.xml.template"
echo "DIST-01 smoke passed."
