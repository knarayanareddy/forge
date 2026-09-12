# AGENTS.md

## Cursor Cloud specific instructions

This repo (`forge` / AetherForge) is a Rust workspace (`Cargo.toml`, `crates/`, `tests/golden_harness`) plus a macOS-only Swift app (`macos/`, `Package.swift`). Cloud agents run on **Linux**, so the relevant scope is the Rust workspace + golden harness. Darwin is the canonical platform.

### Services / components
- **`aether-daemon`** — the app. TCP JSON-lines server on `127.0.0.1:7433`. Run with `cargo run -p aether-daemon`. See `docs/DAEMON_IPC.md`.
- **`golden-harness`** (`tests/golden_harness`) — 54-task acceptance eval. Run with `cargo run -p golden-harness`.
- **macOS Swift app** (`swift build` / `swift run AetherForgeApp` / `scripts/build-ffi.sh`) — **cannot run on Linux** (no Swift toolchain here); skip it.

### Build / test / lint / run (standard commands live in `README.md`, `.github/workflows/ci.yml`, `docs/LINUX_CI.md`)
- Build: `cargo build --workspace` · Tests: `cargo test --workspace`.
- Static lint gate (the only one CI runs): `scripts/scan-mcp-allowlist.sh`. CI does **not** run `cargo clippy` or `cargo fmt`.
- Run app: `cargo run -p aether-daemon`.

### Non-obvious caveats
- **Toolchain:** a transitive dep needs Rust edition2024, so **Rust ≥ 1.85 is required** (build fails on older). The base image's default `rustup` toolchain may be pinned to an old version; the update script runs `rustup default stable`.
- **Golden harness on default Linux scores 41/54 (30 hard-green / 11 soft-green) — this is correct, not a regression.** `FS-02`/`SB-01`/`DIST-01` require Darwin `sandbox-exec`/codesign; `ROUT-01`, `MEM-01`, `GRAPH-01`, `GRAPH-02`, `LOOP-02`, `PLAN-01`, `LOOP-04`, and `INGEST-01` require Darwin/Ollama and print `FAIL-CLOSED`; `MCP-01`/`MCP-02` currently fail the pinned entry-script hash because `npm install -g @modelcontextprotocol/server-filesystem` drifted upstream — re-pin only after reviewing the new build. With local Ollama and a matching MCP pin the score is 51/54. The three newest tasks — `CHECK-02`, `GATE-03`, `RED-02` — are deterministic (no model, no network) and pass on both platforms (measured on Linux in run `34712522700`, PR #52). See `docs/LINUX_CI.md`.
- **Session logs:** every structured-loop execution path (`run_task`, automation triggers, gateway inbound) appends a JSONL transcript under `AETHER_SESSION_LOG_DIR` (default `~/.aether/sessions`) via `aether_daemon::session_log`. `SESS-01` overrides that env var for its own duration only.
- **Git tool children use a fixed local identity** injected by `ProductionSandbox`; host/global git identity is not inherited.
- **Daemon IPC auth:** on non-macOS, auth is optional (no Keychain) unless `AETHER_DAEMON_AUTH_TOKEN` is set. `ping` never needs auth.
- **Loop plan JSON field is `"action"`**, not `"tool"` (one `README.md` snippet is stale). After an `fs_write`, the verify shell blocks `done` until both a `verify_contains` and a lint step have succeeded. A known-good hello-world plan:
  `{"loop":[{"action":"fs_write","path":"hello.txt","content":"forge"},{"action":"verify_contains","path":"hello.txt","text":"forge"},{"action":"python_lint","source":"def ok():\n    return 1\n"},{"action":"done"}]}`
- **A written `.py` artifact must be linted *as that file*** (CHECK-02). `python_lint` compiles source quoted in the plan, so it proves nothing about anything on disk; for every path ending in `.py` that the plan wrote, the gate also requires a successful `python_lint_file` **on that same path** before `done`. A known-good Python plan:
  `{"loop":[{"action":"fs_write","path":"hello.py","content":"def ok():\n    return 1\n"},{"action":"verify_contains","path":"hello.py","text":"def ok"},{"action":"python_lint_file","path":"hello.py"},{"action":"done"}]}`
  There is no `nc` on the box; drive the daemon via a small `python3` socket client (or `cargo run -p aether-daemon --example stream_client`, which needs Ollama).
- **MCP filesystem server** is installed at `/usr/local/lib/node_modules/...` (an auto-discovered candidate path); the harness resolves node via `which node`. `MCP-01` passes on Linux once installed.
- **System deps:** building needs `pkg-config` + `libssl-dev` (openssl). `BYOK` env vars (`AETHER_BYOK_PROVIDER`) fail-closed on Linux — do not set them.
- **Subagent PR-creation gap (lesson learned):** a `best-of-n-runner` subagent dispatched to wire the undo/checkpoint IPC methods into the SwiftUI app (`DaemonClient.swift`, new `SafetyModel.swift`/`SafetyView.swift`, `AetherForgeApp.swift`) reported it had no `ManagePullRequest` tool access, so it could not open its own PR. Its commit ended up carried into `main` as part of an unrelated PR's base rather than reviewed on its own — a real process gap, not a content problem (the code was manually reviewed after the fact and confirmed to build clean on Darwin CI: `main` @ `51658d0`, run `30838281310`, `Build complete!` with only a pre-existing non-sendable-capture warning). **Do not assume a dispatched subagent can create its own PR.** Either verify it has that tool before relying on it, or have the orchestrating agent create the PR itself once the subagent's branch is ready — do not just declare success because the subagent says it committed and pushed.
