# Linux CI Expectations

AetherForge treats **Darwin (macOS 15+)** as the canonical platform. Linux CI validates cross-platform Rust crates and documents explicit fail-closed behavior for platform-specific harness tasks.

**Related:** [ROADMAP_PHASE_6.md](./ROADMAP_PHASE_6.md) · [ROADMAP_PHASE_7.md](./ROADMAP_PHASE_7.md) · [GRAPH_V1.md](./GRAPH_V1.md) · [PHASE_6_SLICE_CHECKLIST.md](./PHASE_6_SLICE_CHECKLIST.md)

## Harness matrix (21 tasks)

| Task | Tier | Linux CI | Reason |
|------|------|----------|--------|
| FS-01 | hard | PASS | Grant-checked `FileMutator` — OS-agnostic |
| FS-02 | hard | **FAIL-CLOSED** | Requires macOS `sandbox-exec` + Seatbelt profile |
| **SB-01** | hard | **FAIL-CLOSED** | Production Seatbelt loop, environment scrubbing, and network-deny gate; Darwin only |
| GIT-01 | hard | PASS | Real git subprocess with grant gate |
| CODE-01 | hard | PASS | `python3 -m py_compile` |
| MCP-01 | hard | PASS* | Requires Node + MCP server installed in CI |
| ROUT-01 | hard | **FAIL-CLOSED** | Requires live Ollama SSE streaming |
| MEM-01 | hard | **FAIL-CLOSED** | Requires Ollama `all-minilm` embeddings |
| **MEM-02** | hard | PASS | Deterministic production daemon chunk→link→isolated-recall path |
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
| Darwin + Ollama + sandbox-exec | **21/21 target** | **21 hard / 0 soft** | Last verified main baseline: 20/20; SB-01 canonical verification pending |
| Linux (default CI) | **14/21** | 14 hard / 0 soft† | FS-02, SB-01, MEM-01, ROUT-01, GRAPH-01, LOOP-02, PLAN-01 fail-closed |
| Linux + Ollama + MCP | **18/21** | 18 hard / 0 soft† | Verified on the SB-01 branch; FS-02, SB-01, and OS-gated LOOP-02 fail closed |

† Fail-closed tasks print `FAIL-CLOSED` and do not inflate the pass count — the harness reports **Passed: 14 / 21** on default Linux CI, not 21/21.

**Do not claim 21/21 on Linux.** Seven tasks require unavailable/default-disabled prerequisites; they must show explicit `FAIL-CLOSED`, never silent skip.

## CI workflow tiers

GitHub Actions (`.github/workflows/ci.yml`):

| Trigger | Linux job | Darwin job |
|---------|-----------|------------|
| **Pull request** | Full harness · gate ≥ 14/21 | **Build + unit tests + Swift only** — no golden harness |
| **Push to `main`** | Full harness · gate ≥ 14/21 | Full harness · gate **21/21 (21 hard)** |
| **Nightly schedule / manual** | Full harness · gate ≥ 14/21 | Full harness · gate **21/21 (21 hard)** |

### PR fast path (Linux Ollama-independent tasks)

PRs validate the Ollama-independent core without blocking on cold-model flake. These **14 tasks** are expected PASS on every Linux run (including PRs):

FS-01, SAFE-01, RES-01, GIT-01, CODE-01, MCP-01, MEM-02, SKILL-01, SKILL-02, RED-01, LOOP-01, AUTO-01, CHECK-01, GATE-01.

Linux PR jobs still run the **full 21-task harness** (14 pass + 7 fail-closed) and gate on ≥ 14/21. **Darwin PR jobs do not run the golden harness** — they run `cargo build`, `cargo test`, MCP allowlist scan, and Swift build only. Merge to `main` or nightly runs enforce **21/21 on Darwin**.

Steps on every job:

1. `cargo build --workspace`
2. `cargo test --workspace`
3. `scripts/scan-mcp-allowlist.sh`
4. `cargo run -p golden-harness` (Linux full matrix; Darwin harness only on push/nightly)

Linux jobs **must not** skip fail-closed tasks silently — harness prints `FAIL-CLOSED` for FS-02, SB-01, MEM-01, ROUT-01, GRAPH-01, LOOP-02, and PLAN-01 when prerequisites are absent.

## ROUT-01 threshold honesty

ROUT-01 measures Ollama's server-side warm TTFT, not end-to-end UI latency:

1. warm the selected model and drain five discard streams;
2. record seven samples;
3. discard the lowest and highest;
4. take the median of the remaining five;
5. retry at most three rounds with re-warming.

The default local Darwin threshold is **200ms**. The GitHub-hosted `macos-15` full-harness job sets
`AETHER_ROUT_TTFT_MS=550` because shared VM scheduling and virtualization add substantial variance.
The 550ms value is a **CI stability allowance**, not the product target, and must never be quoted
as local model performance. Linux without Ollama remains explicit `FAIL-CLOSED`.

## BYOK on Linux

Setting `AETHER_BYOK_PROVIDER` on non-macOS causes daemon startup to **fail closed** (Keychain unavailable). Do not set BYOK env vars in Linux CI.

## Local reproduction

```bash
# Full Darwin gate (SB-01 target — 21/21)
cargo run -p golden-harness
# → Darwin scoreboard: 21/21 harness (21 hard / 0 soft)

# Simulate Linux fail-closed (unset Ollama, non-Darwin only)
# On macOS, FS-02 still passes if sandbox-exec exists.
```

See also [INSTALL.md](./INSTALL.md) for Ollama model requirements and [RATEL_TOOL_INDEX.md](./RATEL_TOOL_INDEX.md) for SKILL-02 progressive-disclosure routing.
