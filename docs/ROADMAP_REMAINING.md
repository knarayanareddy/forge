# AetherForge — Remaining Work Checklist

**Authoritative backlog** for everything not yet shipped against the Phases 9–13 master roadmap.
**Harness registry:** `tests/golden_harness/src/main.rs` (`TaskSpec` array).

**Last updated:** 2026-08-04 · **main @ `164ce2c`** (38 harness tasks merged) · local WIP may target **41** (FORK/HEAD/CACHE probes).

See also: [ROADMAP_PHASES_9-13.md](./ROADMAP_PHASES_9-13.md), [ROADMAP_PHASE_8.md](./ROADMAP_PHASE_8.md), [INSTALL.md](./INSTALL.md), [SPARKLE.md](./SPARKLE.md).

---

## 1. Done on `main` (verified substrate)

| Area | Status |
|------|--------|
| **Phases 1–8.0** | Closed — grants, audit chain, IPC auth, sandbox, graph v1, automations, maker-checker |
| **Phase 9 core** | PLAN-01, SESS-01, UNDO-01, LOOP-04, INGEST-01, BUDG-01 |
| **Phase 10 (narrow)** | HOOK-01 (single PreToolUse denylist), CKPT-01, PERM-02, SUB-01 |
| **Phase 11 (narrow)** | CONS-01, SEC-01, SKILL-03, INJECT-01, GRAPH-02 |
| **Gateways** | GATE-01 (Slack mock), GATE-02 (Telegram/Discord production adapters) |
| **Cost / models** | COST-01 (provider token audit), REG-01 (TOML registry, mlx/gguf fail-closed) |
| **SwiftUI** | Chat, Safety (undo/checkpoint), Memory (consolidation), Subagents, Settings (model picker + BYOK) |
| **Distribution scaffolding** | `create-dmg.sh`, `notarize.sh`, `verify-codesign.sh`, `release.yml` (unsigned default) |

### Harness tasks on `main` (38)

ROUT-01, FS-01, FS-02, SB-01, GIT-01, CODE-01, MCP-01, MEM-01, MEM-02, GRAPH-01, SKILL-01, SKILL-02, SAFE-01, RED-01, RES-01, LOOP-01, LOOP-02, PLAN-01, LOOP-04, SESS-01, UNDO-01, AUTO-01, CHECK-01, GATE-01, GATE-02, HOOK-01, CKPT-01, CONS-01, PERM-02, SUB-01, SEC-01, SKILL-03, INJECT-01, INGEST-01, BUDG-01, GRAPH-02, REG-01, COST-01

**Honesty gap:** Last **cited** Darwin canonical green is **29/29 @ `51658d0`**. Tasks 30–38 merged afterward; a fresh **38/38** (or **41/41** after probes) Darwin run must be executed and cited in README / LINUX_CI.

---

## 2. In flight / aborted (Wave 3 — network failure)

Wave 3 agents were launched 2026-08-04 and **aborted** (`ENOTFOUND` / `PING timed out`) when the Mac went offline. Partial local WIP may exist on branch `docs/roadmap-remaining`:

| Planned slice | Harness | Notes |
|---------------|---------|-------|
| Darwin canonical re-run + doc sync | all | Ollama + full harness, update README matrix |
| Track D — Sparkle + DIST-01 | DIST-01 | Wire update check; CI smoke for codesign/spctl |
| FORK-01 session fork/resume | FORK-01 | JSONL fork over session log |
| HEAD-01 headless JSON | HEAD-01 | NDJSON event stream |
| CACHE-01 prefix cache stability | CACHE-01 | KV/prefix cache probe |
| MCP-02 user-addable MCP | MCP-02 | Pin + diff-on-update trust |
| COMPACT-01 context compaction | COMPACT-01 | Phase 10.5 |
| Hook engine extension | HOOK-01+ | Beyond single PreToolUse rule |
| Phase 12 moat | SLEEP-01, RELY-01, FORENSIC-01, MEM-03 | Pick spec-first items |

**Action:** Re-run wave 3 when Mac is online and awake; land probes **one harness slot per PR** or batch with merge playbook below.

---

## 3. P0 — blocks shippable Mac product

| # | Step | Owner | Effort | Harness / gate |
|---|------|-------|--------|----------------|
| P0-1 | Run **Darwin canonical** full harness; cite run URL in README + LINUX_CI | eng | **S** (0.5–1 d) | 38/38 or 41/41 hard |
| P0-2 | Fix **doc/scoreboard drift** (README still mentions 29/33 in places; align with registry + `check-doc-scoreboard.sh`) | eng | **S** | CI script |
| P0-3 | **Track D:** wire Sparkle into macOS app; appcast template; maintainer EdDSA keygen | eng | **M** (1 wk) | — |
| P0-4 | **DIST-01** harness or CI smoke: `verify-codesign.sh` + `spctl` on release artifact path | eng | **S–M** | DIST-01 |
| P0-5 | **Signed + notarized DMG** with Apple Developer ID (maintainer machine) | maintainer | **M** | manual |
| P0-6 | **SwiftUI ↔ daemon E2E** manual pass: undo, checkpoint, approval, consolidation, settings | QA | **S** (2–3 d) | — |

---

## 4. P1 — table stakes (Phase 9–10 probes)

| # | Step | Harness | Effort |
|---|------|---------|--------|
| P1-1 | Session fork / resume | **FORK-01** | M (1 wk) |
| P1-2 | Headless `--json` NDJSON stream | **HEAD-01** | M (1 wk) |
| P1-3 | KV/prefix cache stability | **CACHE-01** | S–M |
| P1-4 | User-addable MCP + pin + diff-on-update | **MCP-02** | M (1–2 wk) |
| P1-5 | Context compaction | **COMPACT-01** | M |
| P1-6 | Full hook lifecycle engine (8 hooks vs one denylist) | extends HOOK-01 | L (2–3 wk) |
| P1-7 | Permission modes + auto-batching (beyond PERM-02 gate) | PERM-02+ | M |
| P1-8 | User-inspectable memory | **MEM-03** | M |
| P1-9 | Tamper-evident audit export | extends SAFE-01 | M |
| P1-10 | LLM subagent semantic distillation (vs mechanical SUB-01) | SUB-01+ | M |

Master roadmap **scoreboard projection:** **37/37** on core path; probes above promote to hard gates when green.

---

## 5. P2 — moat / Phase 12–13

| # | Step | Harness | Effort |
|---|------|---------|--------|
| P2-1 | Sleep-time memory compute + recall delta | **SLEEP-01** | L (2–4 wk) |
| P2-2 | Per-model tool reliability scores | **RELY-01** | L |
| P2-3 | Failure forensics + regression export | **FORENSIC-01** | L |
| P2-4 | Energy/thermal scheduler | **ENERGY-01** *(probe)* | L |
| P2-5 | Offline degradation matrix | **OFFLINE-01** *(probe)* | M |
| P2-6 | Calibration / abstention | **CAL-01** *(probe)* | M |
| P2-7 | Apple personal-data connectors | **APPLE-01** | XL |
| P2-8 | Voice / vision | **VOICE-01**, **VISION-01** | XL |
| P2-9 | Direct MLX/GGUF inference (power-user backend) | **MLX-01** | L *(optional)* |
| P2-10 | Interop: ACP, Forge-as-MCP-server, OTel | **ACP-01**, **MCPS-01**, **OTEL-01** | M–L |

---

## 6. Track D / E checklists

### Track D — Distribution (parallel, release-blocking)

- [ ] Developer ID Application certificate in Keychain
- [ ] `CODESIGN_IDENTITY` + `AETHER_SIGN=1 ./scripts/create-dmg.sh`
- [ ] `notarytool store-credentials` + `./scripts/notarize.sh`
- [ ] Sparkle EdDSA key + `sign_update` on appcast ([SPARKLE.md](./SPARKLE.md))
- [ ] Fill `formulas/aetherforge.rb.template` after final DMG SHA256
- [ ] GitHub Release workflow with optional sign secrets
- [ ] DIST-01 green in harness or release CI

### Track E — Interop

- [ ] ACP client/server probe
- [ ] Forge exposes MCP server mode
- [ ] OpenTelemetry export for loop/tool spans

---

## 7. Merge order playbook (parallel PRs)

Learned from Waves 2–3 — **never merge two PRs that both append `TaskSpec` without rebasing**:

1. **Merge order:** gateway/backend first → registry/docs → probe batches last.
2. **Harness conflicts:** rebase second PR onto first; assign **sequential task indices** (e.g. GATE-02 = #36, REG-01 = #37, COST-01 = #38, FORK = #39…).
3. **Scoreboard:** in the **same commit** as harness registry change, update:
   - `tests/golden_harness/src/main.rs`
   - `README.md` task list + Darwin line
   - `docs/LINUX_CI.md` matrix
   - `.github/workflows/ci.yml` gate string
   - Run `scripts/check-doc-scoreboard.sh`
4. **Duplicates:** close stale PRs on old base (Wave 2: #28→#29, #30→#32, #33→#31).
5. **macOS drift:** if SwiftUI branch lags `main`, merge `main` into branch before CI.

---

## 8. Explicitly deferred / out of scope

- Graph Leiden community rollups (prefer path-ranking over summarization rollups)
- Kuzu adapter (SQLite sufficient at current scale)
- Discord Gateway WebSocket (HTTP webhook + REST reply only for now)
- Browser automation (Lightpanda) — preconditions unmet
- Agent teams / worktree fleet / LLM-as-judge calibration
- Marketplace / skill publishing
- Cross-model maker-checker (rule-based CHECK-01 only today)
- Pi persona / OmniClaw parity items without harness spec

---

## 9. Current scores (honest)

| Dimension | Score | Notes |
|-----------|-------|-------|
| Harness / eval culture | **8.5 / 10** | 38 tasks on main; strong anti-theater gates |
| Shippable macOS product | **6.5–7 / 10** | Needs Track D + canonical Darwin cite |
| vs scoped roadmap Ph 1–11 | **~7.5 / 10** | Many slices narrower than spec |
| vs mega-prompt Ph 12–13 | **~25–30%** | Moat + personal agent largely unstarted |

**Target:** 8.5+ **foundation** achieved; **8.5+ product** requires P0 + P1 table stakes + Track D.

---

## 10. Suggested next session (copy-paste)

```bash
cd ~/Projects/forge && git pull origin main
ollama serve &   # if not running
cargo run -p golden-harness --bin golden-harness   # target 38/38 or 41/41
./scripts/check-doc-scoreboard.sh
```

Then land in order: **P0-1 doc sync** → **FORK/HEAD/CACHE PR** → **MCP-02/COMPACT-01** → **Track D Sparkle** → **Phase 12 probes**.
