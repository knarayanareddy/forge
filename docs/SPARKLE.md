# Sparkle auto-update setup (Track D.3 — stub)

Sparkle 2.x is **not wired into the SwiftUI app yet**. This document is the maintainer checklist for when `AetherForge.app` is signed and notarized. Follow these steps **after** the final DMG bytes are frozen — EdDSA signatures cover exact file bytes; re-zipping or re-signing after appcast generation silently breaks updates.

## Prerequisites

- Signed + notarized `AetherForge.app` (see [INSTALL.md](./INSTALL.md))
- Sparkle 2.x added to the Xcode/SwiftPM project (future slice)
- A **Sparkle EdDSA private key** stored in Keychain (never commit)

## 1. Generate EdDSA keys (once)

Use Sparkle's `generate_keys` tool (from the Sparkle release bundle or DerivedData build products):

```bash
# After adding Sparkle via SPM or vendoring the release:
# Path varies — search DerivedData or vendor tools under packaging/sparkle/
./generate_keys
```

This creates:

| Key | Storage | Purpose |
|-----|---------|---------|
| **EdDSA private** | macOS Keychain (`sparkle.ed25519`) | Sign update archives (`sign_update`) |
| **EdDSA public** | Embed in `Info.plist` as `SUPublicEDKey` | App verifies appcast signatures |

Record the public key string for `Info.plist`:

```xml
<key>SUFeedURL</key>
<string>https://github.com/knarayanareddy/forge/releases/download/vVERSION/appcast.xml</string>
<key>SUPublicEDKey</key>
<string>PASTE_BASE64_PUBLIC_KEY_HERE</string>
```

## 2. Vendor `sign_update` (recommended)

`sign_update` normally lives in Xcode DerivedData and breaks CI. Vendor a prebuilt copy:

```bash
mkdir -p packaging/sparkle/bin
# Copy from Sparkle release Tools/sign_update for your arch
cp /path/to/Sparkle/bin/sign_update packaging/sparkle/bin/
chmod +x packaging/sparkle/bin/sign_update
```

## 3. Sign the release archive (last step)

**Order matters:** build DMG → sign → notarize → staple → **then** sign for Sparkle.

```bash
VERSION=0.1.0
DMG="build/dmg/AetherForge-${VERSION}.dmg"

# EdDSA signature over exact DMG bytes (uses Keychain private key)
packaging/sparkle/bin/sign_update "$DMG"
# Output: sparkle:edSignature="..." length="..."
```

Save the `edSignature` and `length` for the appcast item.

## 4. Appcast template

Host `appcast.xml` alongside the release asset (GitHub Releases, S3, etc.):

```xml
<?xml version="1.0" encoding="utf-8"?>
<rss version="2.0" xmlns:sparkle="http://www.andymatuschak.org/xml-namespaces/sparkle">
  <channel>
    <title>AetherForge</title>
    <item>
      <title>Version __VERSION__</title>
      <sparkle:version>__VERSION__</sparkle:version>
      <sparkle:shortVersionString>__VERSION__</sparkle:shortVersionString>
      <pubDate>__RFC822_DATE__</pubDate>
      <enclosure
        url="__DMG_URL__"
        sparkle:edSignature="__ED_SIGNATURE__"
        length="__BYTE_LENGTH__"
        type="application/octet-stream"/>
    </item>
  </channel>
</rss>
```

Verify before publishing:

1. Download the DMG from the exact `url` in the appcast.
2. Confirm byte length matches `length`.
3. Run Sparkle's `generate_appcast` or manual EdDSA verify against the enclosure.

## 5. CI smoke (future DIST-01)

When Sparkle is integrated:

```bash
# Fetch appcast, verify EdDSA on enclosure URL (stub — implement in release.yml)
curl -sfL "$APPCAST_URL" | grep -q 'sparkle:edSignature'
```

## Known traps

| Trap | Mitigation |
|------|------------|
| Re-signing DMG after appcast | Regenerate appcast + `sign_update` from final bytes |
| `sign_update` missing in CI | Vendor under `packaging/sparkle/bin/` |
| Key compromise | Rotate EdDSA keys; bump appcast; ship app update with new `SUPublicEDKey` |
| Unsigned feed | Sparkle requires signed updates when `SUEnableAutomaticChecks` is on |

## Integration stub (SwiftPM — not implemented)

Future app changes (do not merge until Sparkle dep is added):

```swift
// Package.swift — dependency stub
// .package(url: "https://github.com/sparkle-project/Sparkle", from: "2.6.0")

// AetherForgeApp.swift — delegate stub
// import Sparkle
// let updaterController = SPUStandardUpdaterController(...)
```

Track progress in [ROADMAP_PHASE_8.md](./ROADMAP_PHASE_8.md) slice **8.12** / Track **D.3**.
