# Linux CI Expectations

AetherForge treats **Darwin (macOS 15+)** as the canonical platform. Linux CI validates cross-platform Rust crates and documents explicit fail-closed behavior for platform-specific harness tasks.

**Related:** [ROADMAP_PHASE_6.md](./ROADMAP_PHASE_6.md) · [ROADMAP_PHASE_7.md](./ROADMAP_PHASE_7.md) · [GRAPH_V1.md](./GRAPH_V1.md) · [PHASE_6_SLICE_CHECKLIST.md](./PHASE_6_SLICE_CHECKLIST.md)

## Harness matrix (19 tasks)

| Task | Tier | Linux CI | Reason |
|------|------|----------|--------|
| FS-01 | hard | PASS | Grant-checked `FileMutator` — OS-agnostic |
| FS-02 | hard | **FAIL-CLOSED** | Requires macOS `sandbox-exec` + Seatbelt profile |
| GIT-01 | hard | PASS | Real git subprocess with grant gate |
| CODE-01 | hard | PASS | `python3 -m py_compile` |
| MCP-01 | hard | PASS* | Requires Node + MCP server installed in CI |
| ROUT-01 | hard | **FAIL-CLOSED** | Requires live Ollama SSE streaming |
| MEM-01 | hard | **FAIL-CLOSED** | Requires Ollama `all-minilm` embeddings |
| GRAPH-01 | hard | **FAIL-CLOSED** | Requires Ollama embeddings + graph recall@k |
| SKILL-01 | hard | PASS | Procedural skill loader |
| SKILL-02 | hard | PASS | Progressive-disclosure routing + citation from fixtures |
| SAFE-01 | hard | PASS | Permission + audit hash chain |
| RED-01 | hard | PASS | Adversarial suite (14 frozen cases); no Ollama dependency |
| RES-01 | hard | PASS | SIGTERM child recovery (uses Unix signals) |
| LOOP-01 | hard | PASS | ReAct loop in production crate |
| LOOP-02 | hard | **FAIL-CLOSED** | Requires Ollama NL planner (`nl_planner`) |
| **PLAN-01** | hard | **FAIL-CLOSED** by default | Requires Ollama; passes on Linux when a chat model is available |
| **AUTO-01** | hard | PASS | Local mock trigger; no Ollama |
| **CHECK-01** | hard | PASS | Rule-based verifier node; no Ollama (FAIL-CLOSED if NL verifier enabled without Ollama) |
| **GATE-01** | hard | PASS | Localhost mock Slack server; no real network |

\* MCP-01 fails if `@modelcontextprotocol/server-filesystem` is not installed — install via `npm install -g` in CI.

## Expected scores

| Environment | Expected harness | Hard / soft | Notes |
|-------------|------------------|-------------|-------|
| Darwin + Ollama + sandbox-exec | **19/19 target** | **19 hard / 0 soft** | PLAN-01 canonical verification pending |
| Linux (default CI) | **13/19** | 13 hard / 0 soft† | FS-02, MEM-01, ROUT-01, GRAPH-01, LOOP-02, PLAN-01 fail-closed |
| Linux + Ollama + MCP | **17/19** | 17 hard / 0 soft† | Verified on the PLAN-01 implementation branch; FS-02 and OS-gated LOOP-02 fail closed |

† Fail-closed tasks print `FAIL-CLOSED` and do not inflate the pass count — the harness reports **Passed: 13 / 19** on default Linux CI, not 19/19.

**Do not claim 19/19 on Linux.** Six tasks require unavailable/default-disabled prerequisites; they must show explicit `FAIL-CLOSED`, never silent skip.

## CI workflow tiers

GitHub Actions (`.github/workflows/ci.yml`):

| Trigger | Linux job | Darwin job |
|---------|-----------|------------|
| **Pull request** | Full harness · gate ≥ 13/19 | **Build + unit tests + Swift only** — no golden harness |
| **Push to `main`** | Full harness · gate ≥ 13/19 | Full harness · gate **19/19 (19 hard)** |
| **Nightly schedule / manual** | Full harness · gate ≥ 13/19 | Full harness · gate **19/19 (19 hard)** |

### PR fast path (Linux Ollama-independent tasks)

PRs validate the Ollama-independent core without blocking on cold-model flake. These **11 tasks** are expected PASS on every Linux run (including PRs):

FS-01, SAFE-01, RES-01, GIT-01, CODE-01, MCP-01, SKILL-01, RED-01, AUTO-01, CHECK-01, GATE-01.

Linux PR jobs still run the **full 19-task harness** (13 pass + 6 fail-closed) and gate on ≥ 13/19. **Darwin PR jobs do not run the golden harness** — they run `cargo build`, `cargo test`, MCP allowlist scan, and Swift build only. Merge to `main` or nightly runs enforce **19/19 on Darwin**.

Steps on every job:

1. `cargo build --workspace`
2. `cargo test --workspace`
3. `scripts/scan-mcp-allowlist.sh`
4. `cargo run -p golden-harness` (Linux full matrix; Darwin harness only on push/nightly)

Linux jobs **must not** skip fail-closed tasks silently — harness prints `FAIL-CLOSED` for FS-02, MEM-01, ROUT-01, GRAPH-01, LOOP-02, and PLAN-01 when prerequisites are absent.

## BYOK on Linux

Setting `AETHER_BYOK_PROVIDER` on non-macOS causes daemon startup to **fail closed** (Keychain unavailable). Do not set BYOK env vars in Linux CI.

## Local reproduction

```bash
# Full Darwin gate (PLAN-01 target — 19/19)
cargo run -p golden-harness
# → Darwin scoreboard: 19/19 harness (19 hard / 0 soft)

# Simulate Linux fail-closed (unset Ollama, non-Darwin only)
# On macOS, FS-02 still passes if sandbox-exec exists.
```

See also [INSTALL.md](./INSTALL.md) for Ollama model requirements and [RATEL_TOOL_INDEX.md](./RATEL_TOOL_INDEX.md) for SKILL-02 progressive-disclosure routing.
