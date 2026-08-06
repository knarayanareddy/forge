#!/usr/bin/env bash
# Verify codesign / notarization on AetherForge release artifacts (Track D.1/D.2).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
# shellcheck source=scripts/lib/distribution.sh
source "$ROOT/scripts/lib/distribution.sh"

distribution_require_darwin "verify-codesign.sh"

ARTIFACT="${1:-}"
if [[ -z "$ARTIFACT" || ! -e "$ARTIFACT" ]]; then
  echo "Usage: $0 <AetherForge.app|AetherForge-x.y.z.dmg>" >&2
  echo "" >&2
  echo "Environment:" >&2
  echo "  AETHER_REQUIRE_NOTARIZED=1   Fail if artifact is signed but not stapled/notarized" >&2
  echo "  AETHER_REQUIRE_SIGNED=1      Fail if artifact is ad-hoc / unsigned (CI release gate)" >&2
  exit 1
fi

REQUIRE_NOTARIZED="${AETHER_REQUIRE_NOTARIZED:-0}"
REQUIRE_SIGNED="${AETHER_REQUIRE_SIGNED:-0}"
FAILURES=0

note_failure() {
  echo "FAIL: $*" >&2
  FAILURES=$((FAILURES + 1))
}

note_ok() {
  echo "OK: $*"
}

verify_app_bundle() {
  local app="$1"
  echo "==> Verifying app: $app"

  local authority
  authority="$(codesign -dv --verbose=4 "$app" 2>&1 | sed -n 's/^Authority=\(.*\)/\1/p' | head -1 || true)"
  if [[ -z "$authority" || "$authority" == "adhoc" ]]; then
    if [[ "$REQUIRE_SIGNED" == "1" ]]; then
      note_failure "Developer ID signature required (found: ${authority:-unsigned})"
    else
      note_ok "unsigned/ad-hoc (acceptable for dev/CI unsigned builds)"
    fi
  else
    if codesign --verify --strict --verbose=2 "$app" 2>&1; then
      note_ok "codesign --verify --strict"
    else
      note_failure "codesign --verify --strict"
    fi
    note_ok "signed by $authority"
  fi

  if [[ "$REQUIRE_NOTARIZED" == "1" ]]; then
    if spctl --assess --type execute -v "$app" 2>&1; then
      note_ok "spctl --assess --type execute"
    else
      note_failure "spctl --assess --type execute (not notarized or Gatekeeper rejected)"
    fi

    if xcrun stapler validate "$app" >/dev/null 2>&1; then
      note_ok "stapler validate (.app)"
    else
      note_failure "stapler validate (.app) — staple ticket missing"
    fi
  fi
}

verify_dmg() {
  local dmg="$1"
  echo "==> Verifying DMG: $dmg"

  if [[ "$REQUIRE_NOTARIZED" == "1" ]]; then
    if xcrun stapler validate "$dmg" >/dev/null 2>&1; then
      note_ok "stapler validate (.dmg)"
    else
      note_failure "stapler validate (.dmg) — staple ticket missing"
    fi
  fi

  local mount_dir
  mount_dir="$(mktemp -d "${TMPDIR:-/tmp}/aetherforge-verify.XXXXXX")"
  cleanup() {
    hdiutil detach "$mount_dir" -quiet >/dev/null 2>&1 || true
    rmdir "$mount_dir" 2>/dev/null || true
  }
  trap cleanup EXIT

  echo "==> Mounting DMG read-only"
  hdiutil attach "$dmg" -mountpoint "$mount_dir" -nobrowse -readonly >/dev/null

  local app=""
  for candidate in "$mount_dir"/*.app; do
    [[ -d "$candidate" ]] || continue
    app="$candidate"
    break
  done

  if [[ -z "$app" ]]; then
    note_failure "no .app found inside DMG"
  else
    verify_app_bundle "$app"
  fi
}

case "$ARTIFACT" in
  *.app)
    verify_app_bundle "$ARTIFACT"
    ;;
  *.dmg)
    verify_dmg "$ARTIFACT"
    ;;
  *)
    note_failure "unsupported artifact type (expected .app or .dmg): $ARTIFACT"
    ;;
esac

if [[ "$FAILURES" -gt 0 ]]; then
  echo "" >&2
  echo "Verification failed with $FAILURES issue(s)." >&2
  exit 1
fi

echo ""
echo "Verification passed."
