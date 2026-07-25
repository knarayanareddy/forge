# AetherForge Install Guide (Phase 6)

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

```bash
chmod +x scripts/create-dmg.sh scripts/notarize.sh
./scripts/create-dmg.sh
# Output: build/dmg/AetherForge-0.1.0.dmg
```

**Signing & notarization** (requires Apple Developer ID):

1. Sign the `.app` inside staging before `hdiutil create`:
   ```bash
   codesign --force --deep --sign "Developer ID Application: Your Name (TEAMID)" build/dmg/staging/AetherForge.app
   ```
2. Re-create DMG if needed, then:
   ```bash
   xcrun notarytool store-credentials AetherForge-notary \
     --apple-id "you@example.com" --team-id TEAMID --password "@keychain:AC_PASSWORD"
   ./scripts/notarize.sh build/dmg/AetherForge-0.1.0.dmg
   ```

Without a Developer certificate, use ad-hoc distribution from source (`swift run` + `cargo run`).

### Offline consolidate (Phase 6)

Review wiki-zone duplicates without auto-apply:

```bash
./scripts/consolidate_memory.sh --session-id <session> --dry-run
# → consolidation_runs.status = review_pending until explicit apply
```

See [GRAPH_V1.md](GRAPH_V1.md) for the three-zone model and review workflow.

## Linux (CI / development)

Darwin is canonical. On Linux, the golden harness **fail-closes** tasks that require macOS sandbox or local Ollama:

| Task | Linux expectation |
|------|-------------------|
| FS-01, GIT-01, CODE-01, MCP-01, SKILL-01, SKILL-02, SAFE-01, RED-01, RES-01, LOOP-01 | PASS |
| FS-02 | FAIL-CLOSED (no `sandbox-exec`) |
| MEM-01, ROUT-01, GRAPH-01, LOOP-02 | FAIL-CLOSED (Ollama absent in CI) |

Expected score: **10/15 PASS**, 5 explicit fail-closed.

See [LINUX_CI.md](LINUX_CI.md) for CI matrix details and PR fast-path vs nightly Darwin gate.

## Verify install

```bash
cargo run -p golden-harness
# Darwin with Ollama warm: 15/15 harness (15 hard / 0 soft)
swift build
./scripts/build-ffi.sh
```

**Ollama flake:** If ROUT-01 passes but GRAPH-01 or LOOP-02 fail, re-run after the harness pre-warm lines complete, or pull models manually (`ollama pull all-minilm qwen2.5:3b`).

Related docs: [ROADMAP_PHASE_6.md](ROADMAP_PHASE_6.md) · [RATEL_TOOL_INDEX.md](RATEL_TOOL_INDEX.md)
