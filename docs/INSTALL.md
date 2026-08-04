# AetherForge Install Guide (Phase 7 complete)

## macOS (canonical)

### Requirements

- macOS 15+ (Apple Silicon recommended)
- [Ollama](https://ollama.com) with `all-minilm` (embeddings) and a chat model such as `qwen2.5:3b` (ROUT-01, GRAPH-01, LOOP-02)
- Node.js + `@modelcontextprotocol/server-filesystem` (for MCP harness / optional tools)
- Rust toolchain + Swift 6 (`swift build`)

### From source (single-app launch)

```bash
git clone https://github.com/knarayanareddy/forge.git
cd forge
cargo build --release -p aether-daemon
./scripts/build-ffi.sh release
swift build -c release
```

Launch the app only — it spawns `aether-daemon` automatically if ping fails:

```bash
swift run AetherForgeApp
```

The app reads the daemon auth token from Keychain (service `AetherForge`, account `daemon-auth-token`). The daemon generates this token on first startup. Bundled `.app` builds include `aether-daemon` at `Contents/MacOS/aether-daemon`; dev builds search `target/debug/aether-daemon` or honor `AETHER_DAEMON_BIN`.

Select a workspace folder during onboarding. Prompts stream tokens from the daemon over TCP JSON-lines in real time.

### Manual two-terminal mode (optional)

```bash
# Terminal 1
cargo run -p aether-daemon

# Terminal 2
swift run AetherForgeApp
```

### BYOK (Bring Your Own Key)

API keys are stored in the **macOS Keychain** (never in plaintext env files):

```rust
// Programmatic store (macOS only):
use aether_core::store_byok_key;
store_byok_key("sk-...")?;
```

```bash
export AETHER_BYOK_PROVIDER=openai
export AETHER_BYOK_MODEL=gpt-4o-mini   # optional
cargo run -p aether-daemon
```

On non-macOS platforms, BYOK is **fail-closed** if `AETHER_BYOK_PROVIDER` is set.

Keychain entry: service `AetherForge`, account `byok-api-key`.

### DMG distribution

Unsigned DMGs work today for local testing. Signed + notarized builds require an **Apple Developer Program** membership and a **Developer ID Application** certificate.

#### Quick path (unsigned, no Apple creds)

```bash
chmod +x scripts/create-dmg.sh scripts/notarize.sh scripts/sign-app.sh scripts/verify-codesign.sh
./scripts/create-dmg.sh
# → build/dmg/AetherForge-0.1.0.dmg (+ .sha256 sidecar)
./scripts/verify-codesign.sh build/dmg/AetherForge-0.1.0.dmg
```

Set `AETHER_VERSION=0.2.0` to override the bundle/DMG version.

#### Signed + notarized path (maintainer machine)

| Requirement | Purpose |
|-------------|---------|
| Apple Developer Program | Issue Developer ID Application cert |
| Developer ID Application cert in Keychain | Hardened Runtime code signing |
| App-specific password + `notarytool store-credentials` | Apple notarization API |
| Full Disk Access for Terminal/Cursor *(if codesign fails with errSecInternalComponent)* | Keychain access for signing |

**One-time notarytool setup:**

```bash
xcrun notarytool store-credentials AetherForge-notary \
  --apple-id "you@example.com" \
  --team-id TEAMID \
  --password "@keychain:AC_PASSWORD"
```

**Build, sign, notarize, verify:**

```bash
export AETHER_VERSION=0.1.0
export CODESIGN_IDENTITY="Developer ID Application: Your Name (TEAMID)"

# Sign inside-out (never codesign --deep) + DMG
AETHER_SIGN=1 ./scripts/create-dmg.sh

DMG="build/dmg/AetherForge-${AETHER_VERSION}.dmg"
APP="build/dmg/staging/AetherForge.app"

# Staples both DMG and .app; skips gracefully if profile/cert missing
./scripts/notarize.sh "$DMG" "$APP"

# Strict gate for release candidates
AETHER_REQUIRE_SIGNED=1 AETHER_REQUIRE_NOTARIZED=1 ./scripts/verify-codesign.sh "$DMG"
```

Entitlements live at `packaging/entitlements/AetherForge.entitlements` (Hardened Runtime + Rust/FFI helpers). Re-sign a staged bundle alone with `./scripts/sign-app.sh build/dmg/staging/AetherForge.app`.

Environment variables:

| Variable | Purpose |
|----------|---------|
| `AETHER_VERSION` | Bundle/DMG version (default `0.1.0`) |
| `AETHER_SIGN=1` | Sign when Developer ID cert is present; warn and continue unsigned if missing |
| `CODESIGN_IDENTITY` | Override auto-detected Developer ID Application identity |
| `NOTARY_PROFILE` | notarytool Keychain profile (default `AetherForge-notary`) |
| `AETHER_SKIP_NOTARIZE=1` | Skip notarization |
| `AETHER_REQUIRE_SIGNED=1` | Fail verify if artifact is unsigned |
| `AETHER_REQUIRE_NOTARIZED=1` | Fail verify if not stapled / Gatekeeper rejects |

#### CI / GitHub Actions

- **Main CI** (`.github/workflows/ci.yml`) does not require Apple certificates.
- **Release** (`.github/workflows/release.yml`, `workflow_dispatch`) builds an unsigned DMG by default and uploads it as an artifact.
- Optional workflow inputs: `sign` (Developer ID in runner Keychain), `notarize` (notarytool profile in Keychain). GitHub-hosted runners have neither — use a self-hosted macOS runner or sign locally.
- No repository secrets are required for the default unsigned release path.

#### Homebrew cask

Fill `formulas/aetherforge.rb.template` with the released version, SHA256 (printed by `create-dmg.sh` or the `.sha256` sidecar), and download URL, then publish to a Homebrew tap.

#### Sparkle updates

Not wired in-app yet. See [SPARKLE.md](SPARKLE.md) for EdDSA keygen, `sign_update` vendoring, and appcast ordering.

Without a Developer certificate, use ad-hoc distribution from source (`swift run` + `cargo run`) or an unsigned DMG.

### Offline consolidate (Phase 6)

Review wiki-zone duplicates without auto-apply:

```bash
./scripts/consolidate_memory.sh --session-id <session>
# → preview artifact + consolidation_runs.status = review_pending
# Apply/reject workflow is not implemented yet.
```

See [GRAPH_V1.md](GRAPH_V1.md) for the three-zone model and review workflow.

## Linux (CI / development)

Darwin is canonical. On Linux, the golden harness **fail-closes** tasks that require macOS sandbox or local Ollama:

| Task | Linux expectation |
|------|-------------------|
| FS-01, GIT-01, CODE-01, MCP-01, MEM-02, SKILL-01, SKILL-02, SAFE-01, RED-01, RES-01, LOOP-01, SESS-01, UNDO-01, AUTO-01, CHECK-01, GATE-01, HOOK-01, CKPT-01, CONS-01, PERM-02, SUB-01 | PASS |
| FS-02, SB-01 | FAIL-CLOSED (Darwin Seatbelt required) |
| MEM-01, ROUT-01, GRAPH-01, LOOP-02, PLAN-01, LOOP-04 | FAIL-CLOSED (Ollama/Darwin unavailable in default CI) |

Expected score: **24/33 PASS**, 8 explicit fail-closed.

See [LINUX_CI.md](LINUX_CI.md) for CI matrix details and PR fast-path vs nightly Darwin gate.

## Verify install

```bash
cargo run -p golden-harness
# Darwin with Ollama warm: 33/33 target (32 hard / 0 soft)
swift build
./scripts/build-ffi.sh
```

**Ollama flake:** If ROUT-01 passes but GRAPH-01 or LOOP-02 fail, re-run after the harness pre-warm lines complete, or pull models manually (`ollama pull all-minilm qwen2.5:3b`).

Related docs: [ROADMAP_PHASE_6.md](ROADMAP_PHASE_6.md) · [ROADMAP_PHASE_7.md](ROADMAP_PHASE_7.md) · [GATEWAY.md](GATEWAY.md) · [RATEL_TOOL_INDEX.md](RATEL_TOOL_INDEX.md) · [SPARKLE.md](SPARKLE.md)
