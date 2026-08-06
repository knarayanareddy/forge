# Linux CI Expectations

AetherForge treats **Darwin (macOS 15+)** as the canonical platform. Linux CI validates cross-platform Rust crates and documents explicit fail-closed behavior for platform-specific harness tasks.

**Related:** [ROADMAP_PHASE_6.md](./ROADMAP_PHASE_6.md) · [ROADMAP_PHASE_7.md](./ROADMAP_PHASE_7.md) · [GRAPH_V1.md](./GRAPH_V1.md) · [PHASE_6_SLICE_CHECKLIST.md](./PHASE_6_SLICE_CHECKLIST.md)

## Harness matrix (51 tasks)

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
| **LOOP-04** | hard | **FAIL-CLOSED** by default | Requires Ollama for the replan step; passes on Linux when a chat model is available |
| **SESS-01** | hard | PASS | Deterministic JSONL session log via `execute_structured_loop`; no Ollama dependency |
| **UNDO-01** | hard | PASS | Undo journal restores multi-file + git run via `execute_structured_loop`; no Ollama dependency |
| **AUTO-01** | hard | PASS | Local mock trigger; no Ollama |
| **CHECK-01** | hard | PASS | Rule-based verifier node; no Ollama (FAIL-CLOSED if NL verifier enabled without Ollama) |
| **GATE-01** | hard | PASS | Localhost mock Slack server; no real network |
| **GATE-02** | hard | PASS | Localhost mock Telegram server; no real network |
| **HOOK-01** | hard | PASS | `PreToolUse` path-denylist hook over `execute_structured_loop`; no Ollama dependency |
| **CKPT-01** | hard | PASS | Checkpoint + rewind over `execute_structured_loop` and the on-disk session log; no Ollama dependency |
| **CONS-01** | hard | PASS | Consolidation apply/reject on in-memory SQLite; no Ollama dependency |
| **PERM-02** | hard | PASS | Pure-function approval gate (`evaluate_approval_gate`) plus `execute_structured_loop`; no Ollama dependency |
| **SUB-01** | hard | PASS | Subagent delegation over `execute_structured_loop`; no Ollama dependency (distillation is mechanical, not LLM-generated) |
| **SEC-01** | hard | PASS | Brokered secret injected at MCP spawn; value absent from plan/context, session log, audit log, and crash dump; no Ollama dependency |
| **SKILL-03** | hard | PASS | Poisoned-skill corpus (≥8) blocked by install/admit/execute trust gate (manifest + pin + injection scan); no Ollama dependency |
| **INJECT-01** | hard | PASS | Tool-result induction corpus (≥8) blocked by cross-call correlation (`admit_plan_against_observations`); no Ollama dependency |
| **INGEST-01** | hard | **FAIL-CLOSED** | Live Ollama `graph_extract` on fresh transcript; no seed replay |
| **BUDG-01** | hard | PASS | Token budget cap enforced in loop engine; no Ollama dependency |
| **COST-01** | hard | PASS | Provider token accounting across loop and daemon paths; no Ollama dependency |
| **GRAPH-02** | hard | **FAIL-CLOSED** | Graph v2 multi-hop recall delta over GRAPH-01 baseline; requires Ollama embeddings |
| **REG-01** | soft | PASS‡ | Model registry load + routing smoke; soft on Darwin (hard off Darwin) |
| **SLEEP-01** | soft | PASS‡ | Sleep-time memory compute with measured recall delta; no Ollama dependency |
| **RELY-01** | soft | PASS‡ | BFCL-style frozen tool reliability corpus + registry scores |
| **FORENSIC-01** | soft | PASS‡ | Session-log failure classifier + regression export corpus |
| **REG-01** | soft | PASS‡ | Model registry load + routing smoke; soft on Darwin |
| **FORK-01** | hard | PASS | Session fork helpers; no Ollama dependency |
| **HEAD-01** | hard | PASS | Headless NDJSON helpers; no Ollama dependency |
| **CACHE-01** | hard | PASS | Prefix-cache fingerprint helpers; no Ollama dependency |
| **DIST-01** | hard | **FAIL-CLOSED** | Darwin codesign + spctl release gates |
| **MCP-02** | soft | PASS‡ | User-addable MCP with pin-on-install and diff-on-update |
| **COMPACT-01** | soft | PASS‡ | Context compaction with thrashing guard |
| **HOOK-02** | soft | PASS‡ | Extended hook lifecycle beyond PreToolUse denylist |
| **MEM-03** | soft | PASS‡ | User-inspectable memory list/edit/delete/export |
| **MCPS-01** | soft | PASS‡ | Forge MCP server stdio stub (`forge_ping`) |
| **OFFLINE-01** | soft | PASS‡ | Ollama offline degradation matrix fails fast with clear messages |

* MCP-01 fails if `@modelcontextprotocol/server-filesystem` is not installed — install via `npm install -g` in CI.

‡ REG-01, SLEEP-01, RELY-01, FORENSIC-01, COMPACT-01, HOOK-02, MEM-03, MCPS-01, and OFFLINE-01 are **soft green** on Darwin; counted in pass total but not in hard-green gate.

## Expected scores

| Environment | Expected harness | Hard / soft | Notes |
|-------------|------------------|-------------|-------|
| Darwin + Ollama + sandbox-exec | **51/51 target** | **41 hard / 10 soft** | Canonical Darwin gate: REG-01, SLEEP-01, RELY-01, FORENSIC-01, MCP-02, COMPACT-01, HOOK-02, MEM-03, MCPS-01, OFFLINE-01 soft green |
| Linux (default CI) | **36/51** | 29 hard / 9 soft† | FS-02, SB-01, MEM-01, ROUT-01, GRAPH-01, LOOP-02, PLAN-01, LOOP-04, INGEST-01, GRAPH-02, DIST-01 fail-closed |
| Linux + Ollama + MCP | **41/51** | 34 hard / 9 soft† | FS-02, SB-01, and OS-gated tasks fail closed |

† Fail-closed tasks print `FAIL-CLOSED` and do not inflate the pass count — the harness reports explicit partial scores on Linux, not 39/39.

**Do not claim 45/45 on Linux.** Twelve tasks require unavailable/default-disabled prerequisites; they must show explicit `FAIL-CLOSED`, never silent skip.

## CI workflow tiers

GitHub Actions (`.github/workflows/ci.yml`):

| Trigger | Linux job | Darwin job |
|---------|-----------|------------|
| **Pull request** | Full harness · gate ≥ 30/51 | **Build + unit tests + Swift only** — no golden harness |
| **Push to `main`** | Full harness · gate ≥ 30/51 | Full harness · gate **51/51 (41 hard / 10 soft)** |
| **Nightly schedule / manual** | Full harness · gate ≥ 30/51 | Full harness · gate **51/51 (41 hard / 10 soft)** |

### PR fast path (Linux Ollama-independent tasks)

PRs validate the Ollama-independent core without blocking on cold-model flake. These **39 tasks** are expected PASS on every Linux run (including PRs):

FS-01, SAFE-01, RES-01, GIT-01, CODE-01, MCP-01, MEM-02, SKILL-01, SKILL-02, RED-01, LOOP-01, SESS-01, UNDO-01, AUTO-01, CHECK-01, GATE-01, GATE-02, HOOK-01, CKPT-01, CONS-01, PERM-02, SUB-01, SEC-01, SKILL-03, INJECT-01, BUDG-01, COST-01, REG-01, SLEEP-01, RELY-01, FORENSIC-01, FORK-01, HEAD-01, CACHE-01, MCP-02, COMPACT-01, HOOK-02, MEM-03, MCPS-01, OFFLINE-01.

Linux PR jobs still run the **full 51-task harness** (39 pass + 12 fail-closed) and gate on ≥ 30/51. **Darwin PR jobs do not run the golden harness** — they run `cargo build`, `cargo test`, MCP allowlist scan, and Swift build only. Merge to `main` or nightly runs enforce **51/51 on Darwin**.

Steps on every job:

1. `cargo build --workspace`
2. `cargo test --workspace`
3. `scripts/scan-mcp-allowlist.sh`
4. `cargo run -p golden-harness` (Linux full matrix; Darwin harness only on push/nightly)

Linux jobs **must not** skip fail-closed tasks silently — harness prints `FAIL-CLOSED` for FS-02, SB-01, MEM-01, ROUT-01, GRAPH-01, LOOP-02, PLAN-01, and LOOP-04 when prerequisites are absent.

## Session logs on Linux

`execute_structured_loop` always appends to a JSONL session log under `AETHER_SESSION_LOG_DIR`
(default `~/.aether/sessions`). SESS-01 overrides this env var for the duration of its own test
only and restores the previous value afterward — it does not touch the real daemon log directory
in CI.

## ROUT-01 threshold honesty

ROUT-01 measures Ollama's server-side warm TTFT, not end-to-end UI latency:

1. warm the selected model and drain five discard streams;
2. record seven samples;
3. discard the lowest and highest;
4. take the median of the remaining five;
5. retry at most three rounds with re-warming.

The default local Darwin threshold is **200ms**. The GitHub-hosted `macos-15` full-harness job sets
`AETHER_ROUT_TTFT_MS=700` because shared VM scheduling and virtualization add substantial variance.
The 700ms value is a **CI stability allowance**, not the product target, and must never be quoted
as local model performance. Linux without Ollama remains explicit `FAIL-CLOSED`.

## BYOK on Linux

Setting `AETHER_BYOK_PROVIDER` on non-macOS causes daemon startup to **fail closed** (Keychain unavailable). Do not set BYOK env vars in Linux CI.

## Local reproduction

```bash
# Full Darwin gate (51 tasks — 41 hard / 10 soft)
cargo run -p golden-harness
# → Darwin scoreboard: 51/51 harness (41 hard / 10 soft)

# Simulate Linux fail-closed (unset Ollama, non-Darwin only)
# On macOS, FS-02 still passes if sandbox-exec exists.
```

See also [INSTALL.md](./INSTALL.md) for Ollama model requirements and [RATEL_TOOL_INDEX.md](./RATEL_TOOL_INDEX.md) for SKILL-02 progressive-disclosure routing.

### INGEST-01 (live graph extract) — Phase 8.2–8.3

Fresh transcript fixture under `tests/golden_harness/fixtures/ingest01_transcript.json` (no `extract_json` seed).
Production path: `aether_daemon::ingest::ingest_turn_with_graph_extract` runs schema-constrained
Ollama `graph_extract`, inserts namespaced graph nodes, embeds the turn, then recall@1 must
surface the distinctive fact. Fail-closed when Ollama is offline.

