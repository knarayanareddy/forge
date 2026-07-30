# AetherForge Roadmap — Phase 8.0 (Honesty + Closed Loop)

**Status:** **CLOSED.** Darwin canonical **22/22 (22 hard / 0 soft)** on `main` @ `432ace9` —
[run `30565128737`](https://github.com/knarayanareddy/forge/actions/runs/30565128737). See
[PHASE_8_0_CLOSURE.md](./PHASE_8_0_CLOSURE.md) for full evidence.
**Baseline:** MEM-02 merged — Darwin **20/20 harness (20 hard / 0 soft)** · CI green on main
**Canonical platform:** Darwin (macOS 15+ Apple Silicon)  
**Binding spec:** This document is the **mandatory wedge** before any [Phase 8.1+](./ROADMAP_PHASE_8.md) feature surface (DMG, graph v2, MLX). **That wedge is now cleared.**
**External critique response:** Code-grounded audit (2026-07-25) — *"stop baking harness trajectories into production paths and close the daemon trust boundary"* before claiming shippable product.

---

## Phase 8.0 Executive Summary

Phase 8.0 closes the **honesty gap** between what the golden harness proves and what the daemon actually does in production. Phase 7 delivered orchestration, maker-checker, and gateway wedges — all frozen at 18/18. External critique verified that several headline features were **harness-shaped**: NL planner locked to LOOP-02 gold tool order, IPC auth covering only `run_task`, memory ingest without chunk→embed→link→retrieve in `run_task`, Seatbelt on FS-02 harness only, and CI/README drift on ROUT thresholds.

Phase 8.0 is **not** distribution, graph v2, or MLX. It is the **trust-boundary + closed-loop** wedge that makes Phase 8 feature work honest.

**Five bullets for stakeholders:**

1. **De-harness NL planner** — production `validate_nl_plan` checks schema, allowed tools, step cap, forbidden patterns — **not** `LOOP02_GOLD_TOOL_ORDER`. Gold trajectory stays in LOOP-02 harness + `validate_nl_plan_gold_trajectory`.
2. **IPC lockdown** — authenticate **every** TCP IPC method except `ping`; forbid `grant_automation` auto-grant via IPC; never auto-insert write grants from caller-chosen paths without UI grant flow.
3. **Memory closed loop in daemon** — post-turn: chunk → embed → `insert_memory_chunk` → `link_graph_chunk` → retrieve into next-turn context assembly (library APIs today; product wiring missing).
4. **Seatbelt on all tool paths** — extend FS-02 pattern beyond harness; production tool execution uses sandbox profile, not eval-only `sandbox-exec`.
5. **CI/README honesty** — document ROUT-01 local 200ms vs CI `AETHER_ROUT_TTFT_MS` (350–550ms); no scoreboard inflation.

**Critique verdict incorporated verbatim:**

> *"Forge is a serious Phase 1–7 security/integration substrate … not yet the product in your vision. As a shippable macOS agent orchestrator today: ~5.5–6.5/10. As a foundation that could become an 8.5–9 product with the right next cuts: yes — but only if you stop baking harness trajectories into production paths and close the daemon trust boundary."*

**Stop / fix before more features (critique priority order):**

| Priority | Fix | Slice |
|----------|-----|-------|
| P0 | Remove `LOOP02_GOLD_TOOL_ORDER` from production `validate_nl_plan` | **8.0a** |
| P0 | Authenticate every IPC method; never `grant_automation` from IPC | **8.0a** |
| P0 | Wire post-turn chunk → embed → link → retrieve in daemon | **8.0b** |
| P0 | Apply Seatbelt (or ASRT-style) to all tool execution | **8.0c** |
| P1 | CI/README ROUT threshold honesty | **8.0d** |

---

## Global Gates & Honesty Rules (Phase 8.0)

These inherit Phases 1–7 gates and add Phase 8.0-specific rules.

| Gate | Rule |
|------|------|
| **Hard green** | LOOP-03 / IPC-01 probes are **hard** only when production crate code enforces the invariant — not harness-only mocks. |
| **Regression lock** | All **20** prior tasks must remain PASS before claiming Phase 8.0 complete. |
| **No NL theater** | LOOP-02 green means Ollama NL plan **executed** through verify shell with harness gold trajectory — not "production validator enforces gold order." |
| **No IPC theater** | `register_automation` without auth must fail-closed — verified by unit/integration tests, not docs-only. |
| **No memory theater** | MEM-01/GRAPH-01 library greens do not satisfy 8.0b — daemon must retrieve into `run_task` context. |
| **Phase 8.1+ gate** | DMG, graph v2, MLX slices in [ROADMAP_PHASE_8.md](./ROADMAP_PHASE_8.md) **blocked** until 8.0 audit checklist passes. |

---

## Prerequisites

| Prerequisite | Evidence |
|--------------|----------|
| Phase 7 complete | Darwin **18/18 (18 hard / 0 soft)** — `cargo run -p golden-harness` |
| CI green | darwin-full 18/18 @ `ec5f750` |
| Independent Phase 7 audit | ROADMAP_PHASE_7.md checklist ticks |
| External critique triage | NL eval-lock + unauth IPC verified and fixed in 8.0a |

**Do not start Phase 8.1+ until Phase 8.0 is pushed and independently audited.**

**Narrow implementation exception:** Phase 9 planner slices 9.1–9.4 / PLAN-01 were pulled
forward after 8.0a because the de-harnessed probe exposed a directly testable planner defect.
This does **not** waive the 8.0b memory or 8.0c sandbox gates for any other Phase 9 surface.

---

## Architecture Delta

```
Phase 7 (today)                         Phase 8.0 (target)
─────────────────                       ─────────────────

validate_nl_plan                        validate_nl_plan
  └─ LOOP02_GOLD_TOOL_ORDER enforced      └─ schema + allowed tools + cap + forbidden patterns
     (production — eval theater)              (production — real NL)
  LOOP-02 harness                         LOOP-02 harness
  └─ trajectory on observations only        └─ + validate_nl_plan_gold_trajectory on planner output

IPC (TCP 7433)                          IPC (TCP 7433)
  run_task → auth ✓                       ALL methods → auth ✓ (except ping)
  register_automation → no auth ✗         register_automation → auth ✓
  grant_automation flag → auto-grant ✗    grant_automation via IPC → forbidden

post_turn ingest                        post_turn ingest
  conversation + graph extract            + chunk → embed → link → retrieve → context

tool execution                          tool execution
  FS-02 harness sandbox only              Seatbelt on production tool paths (8.0c)
```

---

## Implementation Slices (Ordered, Vertical)

| Slice | Scope | Ship signal | Harness impact |
|-------|-------|-------------|----------------|
| **8.0a** | IPC lockdown + NL planner de-harnessing | Auth on all IPC except `ping`; production NL accepts non-gold order; `grant_automation` forbidden via IPC | LOOP-03 probe (unit); IPC integration tests; LOOP-02 retains gold trajectory in harness |
| **8.0b** | Memory closed loop in daemon | `run_task` assembles bounded, session-isolated context after post-turn embed + graph links | **MEM-02** — implemented |
| **8.0c** | Seatbelt on all tool execution paths | Production FS, git, lint, MCP, skill, and gateway writes use one scrubbed sandbox boundary; profile bundled | **SB-01** — implemented |
| **8.0d** | CI/README honesty | ROUT thresholds documented; README scoreboard matches CI env | None (docs gate) |

**Slice ordering rationale:** security + eval honesty (8.0a) before memory product loop (8.0b) before sandbox expansion (8.0c) before doc truth (8.0d). Phase 8.1 token cap and ingest eval inherit an honest NL + IPC foundation.

---

## Harness Tasks (Phase 8.0 Additions / Probes)

Full suite during 8.0: **18/18 retained** (regression lock). New probes may land before promotion to scoreboard.

| Task | Tier | Acceptance criteria | Evidence path |
|------|------|---------------------|---------------|
| **LOOP-02** *(retained)* | **hard** | NL prompt → Ollama planner → `validate_nl_plan` (production) → `ReActLoopEngine` execute. **Harness** asserts gold trajectory on planner output + execution observations. | `tests/golden_harness/src/loop02.rs` |
| **LOOP-03** *(probe → future hard)* | probe | Production `validate_nl_plan` accepts fs_read-first (or other non-gold) plan JSON without `TrajectoryMismatch`. No Ollama required for unit probe. | `tests/golden_harness/src/loop03.rs`, `nl_planner.rs` unit tests |
| **IPC-01** *(probe → future hard)* | probe | Unauthenticated `register_automation`, `automation_tick`, `automation_run` → `Invalid or missing auth_token`. Authenticated register without `grant_automation` succeeds. `grant_automation: true` → explicit error. | `crates/aether-daemon/src/server.rs` unit tests |

**Retained Phase 1–7 tasks (regression lock):** all 18 tasks — **hard**, unchanged tiers.

---

## Anti-Theater Checklist (What NOT to Claim Green)

| Do NOT claim | Because | Required proof |
|--------------|---------|----------------|
| "NL planning works" | Gold order in production validator = eval cheating | LOOP-03 probe + non-gold unit tests pass |
| "Daemon auth closes localhost gap" | Auth on `run_task` only ≠ trust boundary | IPC-01: all methods except `ping` |
| "Memory closed loop shipped" | post-turn extract without link/retrieve | 8.0b daemon wiring + retrieval in context |
| "Sandboxed agent" | FS-02 harness-only Seatbelt | 8.0c production tool paths |
| "18/18 = shippable product" | Harness ≠ product (~6.0 shipped per critique) | Phase 8.0 + Phase 8 distribution |
| "ROUT-01 ≤200ms on CI" | CI uses higher `AETHER_ROUT_TTFT_MS` | 8.0d README/LINUX_CI honesty |

---

## Critical Findings Addressed (from external critique)

### 1. NL planner eval-locked (production bug) — **8.0a**

> *"`validate_nl_plan` rejects any plan that doesn't match `LOOP02_GOLD_TOOL_ORDER`. Any real NL goal that isn't write→verify→lint→done is rejected."*

**Fix:** Remove gold order from production validator; harness owns trajectory via `validate_nl_plan_gold_trajectory` + LOOP-02 observation asserts.

### 2. Daemon auth only covers run_task — **8.0a**

> *"Unauthenticated `register_automation` + `grant_automation: true` → `automation_registered`. Mitigation today is accidental incompleteness — not a security design."*

**Fix:** Auth gate on all IPC methods except `ping`; forbid `grant_automation` via IPC.

### 3. Memory closed loop is open — **8.0b**

> *"Never: chunk → embed → insert_memory_chunk → link_graph_chunk → retrieve into next turn."*

**Fix:** Wire full loop in daemon `post_turn` + context assembly.

### 4. "18/18" narrow milestone + CI honesty — **8.0d**

> *"PRs skip Darwin harness; CI uses `AETHER_ROUT_TTFT_MS=350` vs README 200ms."*

**Fix:** Document thresholds; darwin-full remains canonical gate.

### 5. Naming oversells — **ongoing**

> *"`ReActLoopEngine`, `OllamaMlx`, 'orchestration graph', 'maker-checker' — all weaker than the names imply."*

**Fix:** Anti-theater docs; iterative rename/defer in Phase 8.1+ where needed.

---

## Success Metrics

| Metric | Target | Measurement |
|--------|--------|-------------|
| **Regression lock** | All 21 tasks PASS on Darwin | `cargo run -p golden-harness` |
| **LOOP-03 probe** | fs_read plan accepted by production validator | `cargo test -p aether-core nl_planner` |
| **IPC-01 probe** | 0 unauthenticated automation IPC success | `cargo test -p aether-daemon ipc_` |
| **8.0b closed loop** | retrieve affects next-turn context | Integration test + audit |
| **8.0c sandbox** | fs_write/MCP paths through Seatbelt | FS-02 pattern extended |
| **Shippable slices** | **4/4 Phase 8.0 slices** merge without breaking prior tasks | Per-slice PR |

---

## Definition of Done (Phase 8.0)

- [x] **8.0a:** Production `validate_nl_plan` free of gold trajectory; IPC auth on all methods except `ping`; `grant_automation` forbidden via IPC
- [x] **8.0b:** Daemon post-turn chunk→embed→link→retrieve wired into streamed `run_task`; graph failure degrades to FTS/vector memory
- [x] **8.0c:** Seatbelt on production FS/git/lint/MCP/skill/gateway execution paths; child environment scrubbed; profile bundled
- [x] **8.0d:** ROUT seven-sample trimmed median, local 200ms target, and CI 550ms allowance documented in README + LINUX_CI.md
- [x] Harness: the 21 Phase 8.0 tasks (through SB-01) remain PASS on Darwin. `golden-harness` is a
      single binary that also now runs SESS-01 (Phase 9 slice 9.5-9.6, added after this checklist
      was written); the practical gate — the full current-count Darwin run reporting all-green —
      passed at **22/22** on `main` @ `432ace9` (run `30565128737`).
- [x] Independent audit checklist below passes
- [x] Commits pushed to `main`

---

## Independent Audit Checklist (Phase 8.0 — pre-Phase-8.1)

- [x] `nl_planner.rs` production path has no `LOOP02_GOLD_TOOL_ORDER` check
- [x] LOOP-02 harness still asserts gold trajectory (harness-only)
- [x] LOOP-03 probe passes (non-gold plan accepted)
- [x] Unauthenticated `register_automation` / `automation_tick` / `automation_run` denied
- [x] `grant_automation: true` on IPC returns explicit error
- [x] Structured execution creates no capability grants; authenticated `grant_workspace` is explicit, idempotent, and audited
- [x] Post-turn ingest links chunks and retrieval feeds next turn (MEM-02)
- [x] Production tool paths use Seatbelt profile; SB-01 covers loop, network deny, environment scrub, and workspace escape
- [x] README documents ROUT local vs CI thresholds (8.0d)
- [x] Phase 1–7 regression tasks still PASS within the verified 20/20 main baseline
- [x] ROADMAP_PHASE_8.md references 8.0 prerequisite and is marked historical where re-scoped

---

## Relationship to Phase 8 (8.1+)

Phase 8.0 **must complete** before:

| Phase 8 slice | Blocked until |
|---------------|---------------|
| 8.1 Loop default token cap | 8.0a IPC + NL honesty |
| 8.2–8.3 INGEST-01 live extract eval | 8.0b closed loop foundation |
| 8.4–8.6 Graph v2 | 8.0b `link_graph_chunk` in production |
| 8.8–8.12 GATE-02 / MLX / DMG | 8.0a IPC + 8.0c sandbox |

See [ROADMAP_PHASE_8.md](./ROADMAP_PHASE_8.md) for distribution, graph v2, and MLX scope after 8.0 audit.

## Relationship to Phases 9–13

[ROADMAP_PHASES_9-13.md](./ROADMAP_PHASES_9-13.md) is the master product roadmap after this wedge. It also **re-scopes Phase 8.1+**: distribution moves to a parallel track, graph v2 defers to Phase 11, and direct MLX downgrades to optional (Ollama is now MLX-backed on Apple Silicon).

**Post-8.0a probe evidence feeding Phase 9:** removing the gold trajectory exposed prompt bias,
missing constrained decoding, and no repair path (3/5 ordinary goals failed). Phase 9 slices
9.1–9.4 subsequently resolved this with per-action schemas, bounded repair, and PLAN-01.

---

## Scoreboard History (Projected)

| Milestone | Harness | Hard | Notes |
|-----------|---------|------|-------|
| Phase 7 complete | 18/18 | 18 | AUTO-01, CHECK-01, GATE-01 |
| Phase 8.0a shipped | 18/18 | 18 | +LOOP-03/IPC-01 probes (not in scoreboard yet) |
| PLAN-01 merged | 19/19 | 19 | Schema-constrained planner + repair |
| Phase 8.0b merged | 20/20 | 20 | +MEM-02 |
| Phase 8.0c implementation | 21/21 target | 21 | +SB-01; Linux live verified 18/21 |
| Phase 8.0d + git sandbox fix | 22/22 target | 22 | +SESS-01 (Phase 9 slice 9.5-9.6); Linux live verified 19/22; fixed `GIT_CONFIG_NOSYSTEM` EPERM found by first post-merge Darwin run |
| **Phase 8.0 complete** | **22/22** | **22** | Canonical Darwin verified — run `30565128737` on `main` @ `432ace9` |
| Phase 8.1+ | 23–26 | 23–26 | Per ROADMAP_PHASE_8 |

---

*Phase 8.0 binding spec · response to external code-grounded critique · 2026-07-26 · slices 8.0a–8.0c implemented*
