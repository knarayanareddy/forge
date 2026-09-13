# Linux CI Expectations

AetherForge treats **Darwin (macOS 15+)** as the canonical platform. Linux CI validates cross-platform Rust crates and documents explicit fail-closed behavior for platform-specific harness tasks.

**Last cited full Darwin run:** **51/51** (42 hard / 9 soft) @ `d38ba6e` — verified locally 2026-08-07; ROUT-01 median warm TTFT 27ms. See [README.md](../README.md#darwin-canonical-verification).

**Related:** [ROADMAP_PHASE_6.md](./ROADMAP_PHASE_6.md) · [ROADMAP_PHASE_7.md](./ROADMAP_PHASE_7.md) · [GRAPH_V1.md](./GRAPH_V1.md) · [PHASE_6_SLICE_CHECKLIST.md](./PHASE_6_SLICE_CHECKLIST.md)

## Harness matrix (60 tasks)

| Task | Tier | Linux CI | Reason |
|------|------|----------|--------|
| FS-01 | hard | PASS | Grant-checked `FileMutator` — OS-agnostic |
| FS-02 | hard | **FAIL-CLOSED** | Requires macOS `sandbox-exec` + Seatbelt profile |
| **SB-01** | hard | **FAIL-CLOSED** | Production Seatbelt loop, environment scrubbing, and network-deny gate; Darwin only |
| GIT-01 | hard | PASS | Real git subprocess with grant gate |
| CODE-01 | hard | PASS | `python3 -m py_compile` |
| MCP-01 | hard | **FAIL*** | Requires Node + MCP server installed in CI; currently fails the entry-script hash pin* |
| ROUT-01 | hard | **FAIL-CLOSED** | Requires live Ollama SSE streaming |
| MEM-01 | hard | **FAIL-CLOSED** | Requires Ollama `all-minilm` embeddings |
| **MEM-02** | hard | PASS | Deterministic production daemon chunk→link→isolated-recall path |
| GRAPH-01 | hard | **FAIL-CLOSED** | Requires Ollama embeddings + graph recall@k |
| SKILL-01 | hard | PASS | Procedural skill loader |
| SKILL-02 | hard | PASS | Progressive-disclosure routing + citation from fixtures |
| SAFE-01 | hard | PASS | Permission + audit hash chain |
| RED-01 | hard | PASS | Adversarial suite (14 frozen cases); no Ollama dependency |
| **RED-02** | hard | PASS | Denial rendering split (principle to remote, full to `audit_log`) over the frozen RED-01 corpus and the production loop; no Ollama dependency |
| RES-01 | hard | PASS | SIGTERM child recovery (uses Unix signals) |
| LOOP-01 | hard | PASS | ReAct loop in production crate |
| LOOP-02 | hard | **FAIL-CLOSED** | Requires Ollama NL planner (`nl_planner`) |
| **PLAN-01** | hard | **FAIL-CLOSED** by default | Requires Ollama; passes on Linux when a chat model is available |
| **LOOP-04** | hard | **FAIL-CLOSED** by default | Requires Ollama for the replan step; passes on Linux when a chat model is available |
| **LOOP-05** | hard | PASS | Replan accounting: a failure no plan can repair costs **0** replans and returns a remedy-bearing error; the terminal paths break before any planner call, so no Ollama dependency |
| **READ-01** | hard | PASS | Every bounded read surface reports its bound and pages: `fs_read` `offset`/`limit`, memory injection, subagent preview; no Ollama dependency |
| **REPLY-01** | hard | PASS | A finished run answers instead of signing off: `LoopRunResult.reply` is validated, presents every artifact it wrote, and reaches the stream, the wire, and the session log; no Ollama dependency |
| **PLAN-02** | hard | PASS | The planner can discover and is told what exists: `fs_list` through the real registry (hook, read grant, remedy-bearing denial, bounded listing) plus the appended capability block stating servers, skills and workspace entries — including their absence; no Ollama dependency |
| **MEM-04** | hard | PASS | Memory is filtered at both ends: the never-store list holds even when the turn asks for the secret, only a user-authored chunk may be `stated`, and a retrieved chunk carrying instructions or privilege claims is dropped **and counted** at read time; frozen embeddings, no Ollama dependency |
| **COMPACT-02** | hard | PASS | Compaction guarded as the trust-boundary crossing it is: the policy preamble is never handed to the summarizer, a summary of untrusted turns inherits `trust="untrusted"`, the rules are re-anchored with a bound report, and the compacted state goes back through `admit_plan_against_observations` so a summarized injection still refuses the replan it induced; closure summarizers, no Ollama dependency |
| **SESS-01** | hard | PASS | Deterministic JSONL session log via `execute_structured_loop`; no Ollama dependency |
| **UNDO-01** | hard | PASS | Undo journal restores multi-file + git run via `execute_structured_loop`; no Ollama dependency |
| **AUTO-01** | hard | PASS | Local mock trigger; no Ollama |
| **CHECK-01** | hard | PASS | Rule-based verifier node; no Ollama (FAIL-CLOSED if NL verifier enabled without Ollama) |
| **CHECK-02** | hard | PASS | Per-artifact post-write lint gate over `execute_structured_loop`; no Ollama dependency |
| **GATE-01** | hard | PASS | Localhost mock Slack server; no real network |
| **GATE-02** | hard | PASS | Localhost mock Telegram server; no real network |
| **GATE-03** | hard | PASS | Gateway reply contract (real reply, journaled artifact, no inbound echo); no real network, no Ollama dependency |
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
| **MCP-02** | soft | **FAIL*** | User-addable MCP with pin-on-install and diff-on-update; currently fails the entry-script hash pin* |
| **COMPACT-01** | soft | PASS‡ | Context compaction with thrashing guard |
| **HOOK-02** | soft | PASS‡ | Extended hook lifecycle beyond PreToolUse denylist |
| **MEM-03** | soft | PASS‡ | User-inspectable memory list/edit/delete/export |
| **MCPS-01** | soft | PASS‡ | Forge MCP server stdio stub (`forge_ping`) |
| **OFFLINE-01** | soft | PASS‡ | Ollama offline degradation matrix fails fast with clear messages |

\* MCP-01 and MCP-02 pin the SHA-256 of the MCP server's entry script (`dist/index.js`) and fail
closed when the installed build differs. On the current `ubuntu-24.04` image, `npm install -g
@modelcontextprotocol/server-filesystem` (node 20.20.2) resolves to a newer build —
computed `729dc8511e779e5cd6640851a74b25283e3af1ca3a7106722f993e864a1d9935` vs pinned
`ac12c0307497ebd1c8e0b0fe4b057165cded007cd7c7afc9aeaa68fefe68eb15` — so both tasks report
`FAIL (Security violation: MCP entry script hash mismatch …)`. This is the supply-chain pin doing
its job against upstream drift, not a harness regression: measured on run `34712522700` (PR #52).
Re-pin only after reviewing the new server build. MCP-01 also fails if
`@modelcontextprotocol/server-filesystem` is not installed at all.

‡ REG-01, SLEEP-01, RELY-01, FORENSIC-01, COMPACT-01, HOOK-02, MEM-03, MCPS-01, and OFFLINE-01 are **soft green** on Darwin; counted in pass total but not in hard-green gate.

## Expected scores

| Environment | Expected harness | Hard / soft | Notes |
|-------------|------------------|-------------|-------|
| Darwin + Ollama + sandbox-exec | **60/60 target** | **50 hard / 10 soft** | Canonical Darwin gate: REG-01, SLEEP-01, RELY-01, FORENSIC-01, MCP-02, COMPACT-01, HOOK-02, MEM-03, MCPS-01, OFFLINE-01 soft green |
| Linux (default CI) | **47/60** | 36 hard / 11 soft† | LOOP-05, READ-01, REPLY-01, PLAN-02, MEM-04 and COMPACT-02 are deterministic, so they pass off Darwin. Last *measured* Linux run: `34756069751` (PR #52, `ubuntu-24.04`) at **46/59** (35 hard / 11 soft) — all four wave-2 tasks (LOOP-05, READ-01, REPLY-01, PLAN-02) plus wave 3's MEM-04 **PASS [hard]**. FS-02, SB-01, MEM-01, ROUT-01, GRAPH-01, LOOP-02, PLAN-01, LOOP-04, INGEST-01, GRAPH-02, DIST-01 fail-closed; MCP-01/MCP-02 fail the entry-script hash pin\* |
| Linux + Ollama + MCP | **57/60** | 45 hard / 12 soft† | Derived: the eight Ollama-gated tasks (ROUT-01, MEM-01, GRAPH-01, GRAPH-02, LOOP-02, PLAN-01, LOOP-04, INGEST-01) plus MCP-01/MCP-02 with a matching pin; FS-02, SB-01, DIST-01 still fail closed |

† Fail-closed tasks print `FAIL-CLOSED` and do not inflate the pass count — the harness reports explicit partial scores on Linux, not 47/47. Hard/soft here is the harness's own runtime `Hard green` / `Soft green` split (47 = 36 + 11), which is not the same as the registry's `hard_on_darwin` flags (50 hard / 10 soft): FORK-01, HEAD-01, CACHE-01, COST-01 and MCP-02 are classified per-run by their own implementations.

**Do not claim 60/60 on Linux.** Thirteen tasks do not pass on the default image — eleven require unavailable/default-disabled prerequisites and must show explicit `FAIL-CLOSED`, never silent skip, and two (MCP-01, MCP-02) fail the entry-script hash pin\*.

## CI workflow tiers

GitHub Actions (`.github/workflows/ci.yml`):

| Trigger | Linux job | Darwin job |
|---------|-----------|------------|
| **Pull request** | Full harness · gate ≥ 30/60 | **Build + unit tests + Swift only** — no golden harness |
| **Push to `main`** | Full harness · gate ≥ 30/60 | Full harness · gate **60/60 (50 hard / 10 soft)** |
| **Nightly schedule / manual** | Full harness · gate ≥ 30/60 | Full harness · gate **60/60 (50 hard / 10 soft)** |

### PR fast path (Linux Ollama-independent tasks)

PRs validate the Ollama-independent core without blocking on cold-model flake. These **49 tasks** are expected PASS on every Linux run (including PRs); 47 of them do pass today — MCP-01 and MCP-02 fail the entry-script hash pin\* until the server build is re-pinned:

FS-01, SAFE-01, RES-01, GIT-01, CODE-01, MCP-01, MEM-02, SKILL-01, SKILL-02, RED-01, RED-02, LOOP-01, LOOP-05, READ-01, REPLY-01, PLAN-02, MEM-04, COMPACT-02, SESS-01, UNDO-01, AUTO-01, CHECK-01, CHECK-02, GATE-01, GATE-02, GATE-03, HOOK-01, CKPT-01, CONS-01, PERM-02, SUB-01, SEC-01, SKILL-03, INJECT-01, BUDG-01, COST-01, REG-01, SLEEP-01, RELY-01, FORENSIC-01, FORK-01, HEAD-01, CACHE-01, MCP-02, COMPACT-01, HOOK-02, MEM-03, MCPS-01, OFFLINE-01.

Linux PR jobs still run the **full 60-task harness** (47 pass · 11 `FAIL-CLOSED` · 2 MCP pin failures\*) and gate on ≥ 30/60. **Darwin PR jobs do not run the golden harness** — they run `cargo build`, `cargo test`, MCP allowlist scan, and Swift build only. Merge to `main` or nightly runs enforce **60/60 on Darwin**.

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

1. warm the selected model and drain eight discard streams;
2. record seven samples;
3. when the model is resident (`/api/ps`), use `prompt_eval_duration` only (exclude warm `load_duration` bookkeeping on Apple Silicon);
4. discard the lowest and highest;
5. take the median of the remaining five;
6. retry at most five rounds with re-warming.

The default local Darwin threshold is **200ms**. The GitHub-hosted `macos-15` full-harness job sets
`AETHER_ROUT_TTFT_MS=700` because shared VM scheduling and virtualization add substantial variance.
The 700ms value is a **CI stability allowance**, not the product target, and must never be quoted
as local model performance. Linux without Ollama remains explicit `FAIL-CLOSED`.

## BYOK on Linux

Setting `AETHER_BYOK_PROVIDER` on non-macOS causes daemon startup to **fail closed** (Keychain unavailable). Do not set BYOK env vars in Linux CI.

## Local reproduction

```bash
# Full Darwin gate (60 tasks — 50 hard / 10 soft)
cargo run -p golden-harness
# → Darwin scoreboard: 60/60 harness (51 hard / 9 soft)

# Or with MCP env + log (see scripts/run-darwin-harness.sh)
./scripts/run-darwin-harness.sh /tmp/golden-final.log
```

Simulate Linux fail-closed: unset Ollama on non-Darwin only. On macOS, FS-02 still passes if `sandbox-exec` exists.

See also [INSTALL.md](./INSTALL.md) for Ollama model requirements and [RATEL_TOOL_INDEX.md](./RATEL_TOOL_INDEX.md) for SKILL-02 progressive-disclosure routing.

### INGEST-01 (live graph extract) — Phase 8.2–8.3

Fresh transcript fixture under `tests/golden_harness/fixtures/ingest01_transcript.json` (no `extract_json` seed).
Production path: `aether_daemon::ingest::ingest_turn_with_graph_extract` runs schema-constrained
Ollama `graph_extract`, inserts namespaced graph nodes, embeds the turn, then recall@1 must
surface the distinctive fact. Fail-closed when Ollama is offline.

