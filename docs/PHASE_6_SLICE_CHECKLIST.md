# Phase 6 Slice PR Checklist

Per-slice PR requirements derived from `docs/ROADMAP_PHASE_6.md` and Spec Kit converge pattern. Every Phase 6 slice PR must satisfy its row before merge.

**Global gates (all slices):** 11/11 Phase 1–5 harness regression lock green on Darwin · no README scoreboard inflation · Linux CI fail-closed rules unchanged unless slice explicitly updates `docs/LINUX_CI.md`.

---

## Slice 6.1 — Graph v1 DDL + CRUD

| Field | Requirement |
|-------|-------------|
| Roadmap ref | ROADMAP_PHASE_6 § DDL Snippets, slice 6.1 |
| Regression harness | 11/11 unchanged |
| Anti-theater row | Do not claim "graph memory shipped" — DB-only |
| Files touched | `crates/aether-db/src/graph.rs`, migration in `lib.rs`, unit tests |
| PR body | Migration idempotent proof · no harness task added |

---

## Slice 6.2 — Hybrid query + 1-hop RRF

| Field | Requirement |
|-------|-------------|
| Roadmap ref | ROADMAP_PHASE_6 § Query path delta, slice 6.2 |
| Regression harness | 11/11 unchanged |
| Anti-theater row | hop=0 parity test vs Phase 5 hybrid |
| Files touched | `search_hybrid_with_graph` in `aether-db`, unit tests |
| PR body | Benchmark or unit proof that graph signal changes ranking when hop=1 |

---

## Slice 6.3 — LoopConfig budget telemetry

| Field | Requirement |
|-------|-------------|
| Roadmap ref | ROADMAP_PHASE_6 § P0 loop guardrails, slice 6.3 |
| Regression harness | LOOP-01 PASS · 11/11 lock |
| Anti-theater row | "Loop budget enforced" — production `ReActLoopEngine`, not harness-only counter |
| Files touched | `loop_engine.rs`, `task_runner.rs`, daemon TCP stream events |
| PR body | `max_tokens` / `tokens_used` / `BudgetExceeded` + audit entry demo |

---

## Slice 6.4 — Ollama graph_extract on ingest

| Field | Requirement |
|-------|-------------|
| Roadmap ref | ROADMAP_PHASE_6 § Architecture Delta, slice 6.4 |
| Regression harness | 11/11 unchanged |
| Anti-theater row | Extraction in daemon ingest path, not harness-only |
| Files touched | `graph_extract.rs`, `ingest.rs`, `docs/GRAPH_V1.md` |
| PR body | Three-zone model documented · failure logged, never silent skip |

---

## Slice 6.5 — GRAPH-01 harness

| Field | Requirement |
|-------|-------------|
| Roadmap ref | ROADMAP_PHASE_6 § GRAPH-01 acceptance |
| Regression harness | 12/12 (11 + GRAPH-01) on Darwin |
| Anti-theater row | recall@3 ≥ 1.0 on frozen gold — not edge counts |
| Files touched | `graph01.rs`, `fixtures/graph_seed.json`, `main.rs` registry |
| PR body | Darwin Ollama warm · Linux FAIL-CLOSED documented |

---

## Slice 6.6 — NlPlanner + LOOP-02

| Field | Requirement |
|-------|-------------|
| Roadmap ref | ROADMAP_PHASE_6 § LOOP-02 acceptance |
| Regression harness | 13/13 on Darwin |
| Anti-theater row | NL plan through same verify shell — no JSON inject-only path |
| Files touched | `nl_planner.rs`, `loop02.rs`, trajectory gold fixture |
| PR body | Frozen tool order assert · ROUT warmup note |

---

## Slice 6.7 — RED-01 adversarial suite

| Field | Requirement |
|-------|-------------|
| Roadmap ref | ROADMAP_PHASE_6 § RED-01 acceptance |
| Regression harness | 14/14 · 0/12+ forbidden actions succeed |
| Anti-theater row | "RED team passed" — full frozen suite, not one symlink test |
| Files touched | `red01.rs` harness, `fixtures/red_team_prompts.json` |
| PR body | Category coverage table · all denials audit-logged |

---

## Slice 6.8 — book_to_skill + SKILL-02

| Field | Requirement |
|-------|-------------|
| Roadmap ref | ROADMAP_PHASE_6 § SKILL-02 acceptance |
| Regression harness | 15/15 on Darwin |
| Anti-theater row | Citation span fuzzy match ≥ 0.9 — not summary theater |
| Files touched | `book_to_skill.sh`, `skill02.rs`, `fixtures/book_skill/` |
| PR body | 3/3 frozen routing questions green |

---

## Slice 6.9 — Offline consolidate

| Field | Requirement |
|-------|-------------|
| Roadmap ref | ROADMAP_PHASE_6 § consolidation_runs |
| Regression harness | 15/15 unchanged |
| Anti-theater row | "Consolidation live" — status=review_pending until human apply |
| Files touched | `consolidate.rs`, `consolidate_memory.sh` |
| PR body | Review artifact JSON sample · no auto-apply code path |

---

## Slice 6.10 — Docs + CI + README scoreboard

| Field | Requirement |
|-------|-------------|
| Roadmap ref | ROADMAP_PHASE_6 § Definition of Done |
| Regression harness | 15/15 Darwin · Linux matrix 10/15 fail-closed |
| Anti-theater row | README matches `cargo run -p golden-harness` output exactly |
| Files touched | `README.md`, `docs/LINUX_CI.md`, `docs/GRAPH_V1.md`, CI matrix |
| PR body | Independent audit checklist ticked |

---

## PR template (copy into description)

```markdown
## Slice
6.X — <title>

## Roadmap
docs/ROADMAP_PHASE_6.md slice 6.X

## Harness
- [ ] Regression lock: __/11 (prior phases)
- [ ] New task (if any): __
- [ ] Darwin: `cargo run -p golden-harness`
- [ ] Linux fail-closed unchanged or documented

## Anti-theater
Which row from ROADMAP § Anti-Theater Checklist does this slice prove?

## Files touched
- 

## Deferrals
What this slice explicitly does NOT claim.
```
