# AGENTS.md

## Cursor Cloud specific instructions

This repo (`forge` / AetherForge) is a Rust workspace (`Cargo.toml`, `crates/`, `tests/golden_harness`) plus a macOS-only Swift app (`macos/`, `Package.swift`). Cloud agents run on **Linux**, so the relevant scope is the Rust workspace + golden harness. Darwin is the canonical platform.

### Services / components
- **`aether-daemon`** — the app. TCP JSON-lines server on `127.0.0.1:7433`. Run with `cargo run -p aether-daemon`. See `docs/DAEMON_IPC.md`.
- **`golden-harness`** (`tests/golden_harness`) — 18-task acceptance eval. Run with `cargo run -p golden-harness`.
- **macOS Swift app** (`swift build` / `swift run AetherForgeApp` / `scripts/build-ffi.sh`) — **cannot run on Linux** (no Swift toolchain here); skip it.

### Build / test / lint / run (standard commands live in `README.md`, `.github/workflows/ci.yml`, `docs/LINUX_CI.md`)
- Build: `cargo build --workspace` · Tests: `cargo test --workspace`.
- Static lint gate (the only one CI runs): `scripts/scan-mcp-allowlist.sh`. CI does **not** run `cargo clippy` or `cargo fmt`.
- Run app: `cargo run -p aether-daemon`.

### Non-obvious caveats
- **Toolchain:** a transitive dep needs Rust edition2024, so **Rust ≥ 1.85 is required** (build fails on older). The base image's default `rustup` toolchain may be pinned to an old version; the update script runs `rustup default stable`.
- **Golden harness on Linux scores 13/18 — this is correct, not a regression.** `FS-02` (needs macOS `sandbox-exec`) and `ROUT-01`, `MEM-01`, `GRAPH-01`, `LOOP-02` (need a local Ollama server) print `FAIL-CLOSED`. Do **not** claim 18/18 on Linux. With a local Ollama service you can reach 17/18 (`FS-02` still fail-closed). See `docs/LINUX_CI.md`.
- **Harness needs git identity env** (for `GIT-01`): export `GIT_AUTHOR_NAME`/`GIT_AUTHOR_EMAIL`/`GIT_COMMITTER_NAME`/`GIT_COMMITTER_EMAIL` (CI uses `ci` / `ci@localhost`), or configure `git config user.*`.
- **Daemon IPC auth:** on non-macOS, auth is optional (no Keychain) unless `AETHER_DAEMON_AUTH_TOKEN` is set. `ping` never needs auth.
- **Loop plan JSON field is `"action"`**, not `"tool"` (one `README.md` snippet is stale). After an `fs_write`, the verify shell blocks `done` until both a `verify_contains` and a `python_lint` step have succeeded. A known-good hello-world plan:
  `{"loop":[{"action":"fs_write","path":"hello.txt","content":"forge"},{"action":"verify_contains","path":"hello.txt","text":"forge"},{"action":"python_lint","source":"def ok():\n    return 1\n"},{"action":"done"}]}`
  There is no `nc` on the box; drive the daemon via a small `python3` socket client (or `cargo run -p aether-daemon --example stream_client`, which needs Ollama).
- **MCP filesystem server** is installed at `/usr/local/lib/node_modules/...` (an auto-discovered candidate path); the harness resolves node via `which node`. `MCP-01` passes on Linux once installed.
- **System deps:** building needs `pkg-config` + `libssl-dev` (openssl). `BYOK` env vars (`AETHER_BYOK_PROVIDER`) fail-closed on Linux — do not set them.
