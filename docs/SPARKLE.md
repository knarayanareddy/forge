# Sparkle auto-update setup (Track D.3)

Sparkle 2.x is wired into the SwiftUI app via SwiftPM. Automatic checks stay off until maintainer EdDSA keys and a signed appcast ship. GitHub-hosted runners do not have Apple Developer ID or Sparkle private keys.

## Honest status

| Item | CI / unsigned build | Maintainer machine |
|------|---------------------|-------------------|
| Sparkle linked | Yes | Yes |
| Auto-update checks | Off (no public key) | On after keygen |
| Developer ID signing | No | Yes |
| Notarization | No | Yes |
| Live appcast | Template only | Fill at release |

See `scripts/dist01-smoke.sh`, harness **DIST-01**, and `.github/workflows/release.yml`.
