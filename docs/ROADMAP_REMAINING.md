# AetherForge — Remaining Work Checklist

**Authoritative backlog** for everything not yet shipped against the Phases 9–13 master roadmap.
**Harness registry:** `tests/golden_harness/src/main.rs` (`TaskSpec` array — **48 tasks**, 42 hard / 6 soft).

**Last updated:** 2026-08-06 · **main @ `372eb37`** (48 harness tasks merged via Wave 3–5 + Phase 12 moat probes).

See also: [ROADMAP_PHASES_9-13.md](./ROADMAP_PHASES_9-13.md), [ROADMAP_PHASE_8.md](./ROADMAP_PHASE_8.md), [INSTALL.md](./INSTALL.md), [SPARKLE.md](./SPARKLE.md).

---

## Project status @ 2026-08-06

| Metric | Value |
|--------|-------|
| **Harness registry** | **48 tasks** (42 hard / 6 soft) |
| **main HEAD** | `372eb37` — Merge PR #44 (Phase 12 moat: SLEEP-01, RELY-01, FORENSIC-01) |
| **Darwin CI gate** | Push/nightly enforces **48/48** (`.github/workflows/ci.yml`) |
| **Last cited Darwin run** | **38/38 @ `6caa521`** (PR #39); tasks 39–48 merged afterward — **fresh 48/48 local cite still needed** |
| **Scoreboard script** | `./scripts/check-doc-scoreboard.sh` — **PASS** (48 tasks) |

### What's truly left for 1.0 release

**Maintainer-only (blocks public install claims):**

1. **Apple Developer ID** — Application certificate in Keychain; `CODESIGN_IDENTITY` + `AETHER_SIGN=1 ./scripts/create-dmg.sh`
2. **Notarized signed DMG** — `notarytool store-credentials` + `./scripts/notarize.sh` on maintainer machine
3. **Sparkle EdDSA** — keygen, `sign_update` on appcast, fill `SUFeedURL` / `SUPublicEDKey` ([SPARKLE.md](./SPARKLE.md))
4. **Homebrew cask** — fill `formulas/aetherforge.rb.template` after final DMG SHA256

**Engineering (small, non-blocking for private beta):**

5. **Cite Darwin 48/48** — run full harness locally; add run URL + commit SHA to README / LINUX_CI (CI already gates 48/48 on main push)
6. **SwiftUI ↔ daemon E2E** — manual pass: undo, checkpoint, approval, consolidation, settings (2–3 d QA)

**Explicitly post-1.0 / deferred:** APPLE-01 (personal-data connectors), MLX-01 (direct inference), marketplace / skill publishing, Track E interop (ACP, Forge-as-MCP-server, OTel), MEM-03, full 8-hook lifecycle engine, ENERGY/OFFLINE/CAL/VOICE/VISION probes.

**Verdict:** Foundation and harness culture are **1.0-ready**. Public “installable product” claims require Track D signing steps above (~1 maintainer day once Apple creds exist).

---

## 1. Done on `main` (verified substrate)

| Area | Status |
|------|--------|
| **Phases 1–8.0** | Closed — grants, audit chain, IPC auth, sandbox, graph v1, automations, maker-checker |
| **Phase 9 core** | PLAN-01, SESS-01, UNDO-01, LOOP-04, INGEST-01, BUDG-01 |
| **Phase 10 (narrow)** | HOOK-01, CKPT-01, PERM-02, SUB-01, **FORK-01**, **HEAD-01**, **CACHE-01**, **MCP-02**, **COMPACT-01**, **HOOK-02** (probe) |
| **Phase 11 (narrow)** | CONS-01, SEC-01, SKILL-03, INJECT-01, GRAPH-02 |
| **Phase 12 (probes)** | **SLEEP-01**, **RELY-01**, **FORENSIC-01** (soft green — fixture-backed, not full moat) |
| **Gateways** | GATE-01 (Slack mock), GATE-02 (Telegram/Discord production adapters) |
| **Cost / models** | COST-01 (provider token audit), REG-01 (TOML registry, mlx/gguf fail-closed) |
| **SwiftUI** | Chat, Safety (undo/checkpoint), Memory (consolidation), Subagents, Settings (model picker + BYOK) |
| **Distribution scaffolding** | Sparkle 2.x linked (SwiftPM), `SparkleUpdateController`, `create-dmg.sh`, `notarize.sh`, `verify-codesign.sh`, `dist01-smoke.sh`, `release.yml` (unsigned default) |
| **Track D gates** | **DIST-01** harness (Darwin hard) + CI smoke on release path (#40) |

### Harness tasks on `main` (48)

ROUT-01, FS-01, FS-02, SB-01, GIT-01, CODE-01, MCP-01, MEM-01, MEM-02, GRAPH-01, SKILL-01, SKILL-02, SAFE-01, RED-01, RES-01, LOOP-01, LOOP-02, PLAN-01, LOOP-04, SESS-01, UNDO-01, AUTO-01, CHECK-01, GATE-01, GATE-02, HOOK-01, CKPT-01, CONS-01, PERM-02, SUB-01, SEC-01, SKILL-03, INJECT-01, INGEST-01, BUDG-01, GRAPH-02, REG-01, COST-01, SLEEP-01, RELY-01, FORENSIC-01, FORK-01, HEAD-01, CACHE-01, DIST-01, MCP-02, COMPACT-01, HOOK-02

**Hard on Darwin (42):** all except REG-01, SLEEP-01, RELY-01, FORENSIC-01, MCP-02, COMPACT-01, HOOK-02.

**Honesty gap:** Last **cited** Darwin canonical green is **38/38 @ `6caa521`** (PR #39). Tasks 39–48 (FORK/HEAD/CACHE, DIST-01, MCP-02/COMPACT-01/HOOK-02, SLEEP/RELY/FORENSIC) merged afterward; CI enforces **48/48** on main push — a fresh local **48/48** run should be executed and cited in README / LINUX_CI.

### Wave 3–5 merge history (closed)

| PR | Slice | Harness impact |
|----|-------|----------------|
| #38 | FORK-01, HEAD-01, CACHE-01 | 41 tasks |
| #39 | Darwin 38/38 verify + doc sync | 38/38 cited |
| #40 | Track D — Sparkle stub + DIST-01 | DIST-01 hard gate |
| #41–#42 | MCP-02, COMPACT-01, HOOK-02 | 45 tasks |
| #43–#44 | SLEEP-01, RELY-01, FORENSIC-01 | **48 tasks** |

---

## 2. ~~In flight / aborted (Wave 3)~~ — **CLOSED 2026-08-06**

Wave 3 agents aborted 2026-08-04 (`ENOTFOUND` offline). All planned slices landed via PRs #38–#44:

| Planned slice | Status | PR |
|---------------|--------|-----|
| Darwin canonical re-run + doc sync | **38/38 cited**; 48/48 CI gate live | #39 |
| Track D — Sparkle + DIST-01 | **Sparkle linked; DIST-01 green**; EdDSA/appcast pending maintainer | #40 |
| FORK-01 session fork/resume | **Done** | #38 |
| HEAD-01 headless JSON | **Done** | #38 |
| CACHE-01 prefix cache stability | **Done** | #38 |
| MCP-02 user-addable MCP | **Done** (soft) | #41–#42 |
| COMPACT-01 context compaction | **Done** (soft) | #41–#42 |
| Hook engine extension | **HOOK-02 probe** (soft); full 8-hook lifecycle still open | #41–#42 |
| Phase 12 moat | **SLEEP/RELY/FORENSIC probes** (soft) | #43–#44 |

---

## 3. P0 — blocks shippable Mac product

| # | Step | Owner | Effort | Harness / gate | Status |
|---|------|-------|--------|----------------|--------|
| P0-1 | Run **Darwin canonical** full harness; cite **48/48** run URL in README + LINUX_CI | eng | **S** (0.5 d) | 48/48 | **Partial** — CI gates 48/48; local cite still 38/38 |
| P0-2 | Fix **doc/scoreboard drift** | eng | **S** | `check-doc-scoreboard.sh` | **Done** — 48-task scoreboard aligned |
| P0-3 | **Track D:** Sparkle in macOS app; appcast template; maintainer EdDSA keygen | maintainer | **S–M** | — | **Partial** — Sparkle linked; keys/appcast pending |
| P0-4 | **DIST-01** harness + CI smoke | eng | **S** | DIST-01 | **Done** (#40) |
| P0-5 | **Signed + notarized DMG** with Apple Developer ID | maintainer | **M** | manual | **Open** — scripts exist; creds required |
| P0-6 | **SwiftUI ↔ daemon E2E** manual pass | QA | **S** (2–3 d) | — | **Open** |

---

## 4. P1 — table stakes (Phase 9–10 probes)

| # | Step | Harness | Effort | Status |
|---|------|---------|--------|--------|
| P1-1 | Session fork / resume | **FORK-01** | M | **Done** (#38) |
| P1-2 | Headless `--json` NDJSON stream | **HEAD-01** | M | **Done** (#38) |
| P1-3 | KV/prefix cache stability | **CACHE-01** | S–M | **Done** (#38) |
| P1-4 | User-addable MCP + pin + diff-on-update | **MCP-02** | M | **Done** (soft, #41–#42) |
| P1-5 | Context compaction | **COMPACT-01** | M | **Done** (soft, #41–#42) |
| P1-6 | Full hook lifecycle engine (8 hooks vs HOOK-01 denylist) | HOOK-02+ | L | **Open** — HOOK-02 probe only |
| P1-7 | Permission modes + auto-batching (beyond PERM-02 gate) | PERM-02+ | M | **Open** |
| P1-8 | User-inspectable memory | **MEM-03** | M | **Open** |
| P1-9 | Tamper-evident audit export | extends SAFE-01 | M | **Open** |
| P1-10 | LLM subagent semantic distillation (vs mechanical SUB-01) | SUB-01+ | M | **Open** |

Master roadmap **scoreboard projection:** **48/48** on core + probe path achieved; remaining P1 items are **quality depth**, not registry gaps.

---

## 5. P2 — moat / Phase 12–13

| # | Step | Harness | Effort | Status |
|---|------|---------|--------|--------|
| P2-1 | Sleep-time memory compute + recall delta | **SLEEP-01** | L | **Probe merged** (soft, #43–#44) — full moat open |
| P2-2 | Per-model tool reliability scores | **RELY-01** | L | **Probe merged** (soft) |
| P2-3 | Failure forensics + regression export | **FORENSIC-01** | L | **Probe merged** (soft) |
| P2-4 | Energy/thermal scheduler | **ENERGY-01** *(probe)* | L | **Open** |
| P2-5 | Offline degradation matrix | **OFFLINE-01** *(probe)* | M | **Open** |
| P2-6 | Calibration / abstention | **CAL-01** *(probe)* | M | **Open** |
| P2-7 | Apple personal-data connectors | **APPLE-01** | XL | **Open** — post-1.0 |
| P2-8 | Voice / vision | **VOICE-01**, **VISION-01** | XL | **Open** |
| P2-9 | Direct MLX/GGUF inference (power-user backend) | **MLX-01** | L | **Open** — optional, post-1.0 |
| P2-10 | Interop: ACP, Forge-as-MCP-server, OTel | **ACP-01**, **MCPS-01**, **OTEL-01** | M–L | **Open** (Track E) |

---

## 6. Track D / E checklists

### Track D — Distribution (parallel, release-blocking)

- [ ] Developer ID Application certificate in Keychain
- [ ] `CODESIGN_IDENTITY` + `AETHER_SIGN=1 ./scripts/create-dmg.sh`
- [ ] `notarytool store-credentials` + `./scripts/notarize.sh`
- [x] Sparkle 2.x linked in SwiftUI app ([SPARKLE.md](./SPARKLE.md))
- [ ] Sparkle EdDSA key + `sign_update` on appcast
- [ ] Fill `formulas/aetherforge.rb.template` after final DMG SHA256
- [x] GitHub Release workflow with optional sign secrets (`release.yml`)
- [x] DIST-01 green in harness + release CI smoke (#40)

### Track E — Interop

- [ ] ACP client/server probe
- [ ] Forge exposes MCP server mode
- [ ] OpenTelemetry export for loop/tool spans

---

## 7. Merge order playbook (parallel PRs)

Learned from Waves 2–5 — **never merge two PRs that both append `TaskSpec` without rebasing**:

1. **Merge order:** gateway/backend first → registry/docs → probe batches last.
2. **Harness conflicts:** rebase second PR onto first; assign **sequential task indices**.
3. **Scoreboard:** in the **same commit** as harness registry change, update:
   - `tests/golden_harness/src/main.rs`
   - `README.md` task list + Darwin line
   - `docs/LINUX_CI.md` matrix
   - `.github/workflows/ci.yml` gate string
   - Run `scripts/check-doc-scoreboard.sh`
4. **Duplicates:** close stale PRs on old base.
5. **macOS drift:** if SwiftUI branch lags `main`, merge `main` into branch before CI.

---

## 8. Explicitly deferred / out of scope

- Graph Leiden community rollups (prefer path-ranking over summarization rollups)
- Kuzu adapter (SQLite sufficient at current scale)
- Discord Gateway WebSocket (HTTP webhook + REST reply only for now)
- Browser automation (Lightpanda) — preconditions unmet
- Agent teams / worktree fleet / LLM-as-judge calibration
- **Marketplace / skill publishing** — post-1.0
- **APPLE-01** personal-data connectors — post-1.0
- **MLX-01** direct inference — optional post-1.0
- Cross-model maker-checker (rule-based CHECK-01 only today)
- Pi persona / OmniClaw parity items without harness spec

---

## 9. Current scores (honest)

| Dimension | Score | Notes |
|-----------|-------|-------|
| Harness / eval culture | **9 / 10** | 48 tasks on main; 42 hard gates + 6 soft probes; anti-theater CI |
| Shippable macOS product | **7.5 / 10** | All code paths + DIST-01 done; **Apple creds + notarized DMG** remain |
| vs scoped roadmap Ph 1–11 | **~8 / 10** | Table-stakes probes merged; depth items (MEM-03, full hooks) open |
| vs mega-prompt Ph 12–13 | **~40%** | SLEEP/RELY/FORENSIC probes in; APPLE-01, MLX-01, marketplace unstarted |

**Target:** **8.5+ foundation achieved.** **7.5+ shippable** for private beta today; **8.5+ product** requires P0-5 signing + P0-6 E2E + cited 48/48 Darwin run.

---

## 10. Suggested next session (copy-paste)

```bash
cd ~/Projects/forge && git pull origin main
ollama serve &   # if not running
cargo run -p golden-harness --bin golden-harness   # target 48/48 (42 hard / 6 soft)
./scripts/check-doc-scoreboard.sh
# Then: cite run in README + LINUX_CI → maintainer signing (P0-5) → Sparkle appcast (P0-3)
```
