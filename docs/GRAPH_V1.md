# Graph v1 — Three-Zone Memory Model

**Phase 6 Slice 6.4** · AetherForge bi-temporal entity graph  
**Binding spec:** [ROADMAP_PHASE_6.md](./ROADMAP_PHASE_6.md)

---

## Overview

Graph v1 is **query-first, not viz-first**. Typed nodes and edges live in SQLite with bi-temporal validity. Hybrid RRF retrieval (FTS5 + sqlite-vec) gains a third signal via 1-hop graph traversal. There is no force-directed UI and no GraphRAG megastack in Phase 6.

---

## Three Zones

AetherForge memory is partitioned into three zones (Obsidian-inspired, SQLite-backed):

| Zone | Tables / artifacts | Writable by agent? | Purpose |
|------|-------------------|--------------------|---------|
| **Raw** | `conversations`, `audit_log` | Append-only transcript + audit trail | Immutable session history; ingest source text |
| **Wiki** | `graph_nodes`, `graph_edges` | Yes (via ingest extract) | Synthesized facts with bi-temporal validity |
| **Schema** | `skills/*/SKILL.md`, `query_policy` | No at runtime | Procedural routing rules and RRF policy |

```
Session turn (daemon post-turn hook)
    │
    ├─► Raw zone: conversations (user + assistant rows)
    │
    └─► Wiki zone: Ollama graph_extract → validate → insert graph_nodes/edges
              │
              └─► Query path: search_hybrid_with_graph (FTS + vec + 1-hop RRF)
```

Agent code **writes rows**, never alters DDL at runtime.

---

## Ingest Pipeline (Slice 6.4)

Post-turn hook runs after stream or ReAct loop completion when `session_id` is present.

1. **Normalize** — whitespace collapse; truncate to `max_tokens_per_batch` (default 4096 estimated tokens).
2. **Extract** — Ollama JSON-mode call with pinned `graph_extract.schema.json` prompt.
3. **Validate** — `validate_graph_extract()`; mandatory `evidence_text` and provenance (`extracted` | `inferred`) on every node/edge.
4. **Cap** — `enforce_max_entities()` rejects payloads exceeding `max_entities_per_turn` (default 32).
5. **Insert** — `payload_to_graph_inserts()` → `insert_graph_node` / `insert_graph_edge` (session-namespaced node ids).

### Failure policy

| Policy | Default | Behavior |
|--------|---------|----------|
| `async_failure_policy` | `LogAndContinue` | Empty/failed turns → `audit_log` (`ingest_turn`, denied) — **never silent skip** |

Empty turns, Ollama failures, validation errors, and insert errors all produce audit entries under tool name `ingest_turn`.

---

## Bi-Temporal Validity

Wiki-zone facts carry two time dimensions:

- **`valid_from` / `valid_to`** — when the fact was true in the world
- **`recorded_at`** — when AetherForge recorded it
- **`superseded_by`** — replacement node id after offline consolidation (Slice 6.9)

Active node query pattern:

```sql
SELECT * FROM graph_nodes
WHERE session_id = ?1
  AND valid_from <= ?2
  AND (valid_to IS NULL OR valid_to > ?2)
  AND superseded_by IS NULL;
```

---

## Query Policy (Schema Zone)

`query_policy` table controls hybrid retrieval weights (not agent-writable):

| Field | Default | Notes |
|-------|---------|-------|
| `rrf_k` | 60.0 | RRF fusion constant |
| `graph_hop_depth` | 1 | 0 = Phase 5 hybrid only; 1 = 1-hop expansion |
| `fts_weight` / `vec_weight` / `graph_weight` | 1.0 each | Signal weights |
| `max_graph_expansion` | 32 | Cap on graph-augmented candidates |

---

## Consolidation Review Workflow (Slice 6.9 — shipped)

Offline `./scripts/consolidate_memory.sh`:

1. Dedupe and resolve contradictions across wiki-zone nodes
2. Write a human-reviewable diff artifact (JSON on disk + markdown preview)
3. Set `consolidation_runs.status = review_pending` — **no auto-apply**

Raw zone is never deleted; supersession uses `superseded_by` only after explicit human approval.

### Review artifact format (pinned pre-6.5, Rowboat backlink borrow)

The consolidate preview schema lives in `crates/aether-db/src/consolidate_review.rs`:

| Type | Fields |
|------|--------|
| `ConsolidatePreview` | `nodes_added`, `nodes_superseded`, `edges_changed`, `contradiction_count`, `dedupe_count` |
| `ConsolidateNodeDiff` | `id`, `canonical_name`, `entity_type`, `action`, `superseded_by`, `source_uri`, `evidence_text` |
| `ConsolidateEdgeDiff` | `src_name`, `dst_name`, `relation_type`, `action`, `evidence_text`, `source_uri` |

`format_consolidate_review(preview)` renders human-readable markdown for review UI:

- Entity names as `[[Entity Name]]` backlinks (Rowboat/Obsidian semantics)
- Provenance per node/edge (`source_uri`)
- Edge changes with relation type and evidence text
- Footer: status must remain `review_pending` until explicit apply

Rowboat uses markdown wiki graphs on disk; AetherForge mirrors the **backlink cross-reference semantics**
in SQLite wiki zone — consolidate preview makes supersession and contradiction resolution readable
before any `superseded_by` mutation is applied.

```rust
use aether_db::{format_consolidate_review, ConsolidatePreview, /* ... */};

let markdown = format_consolidate_review(&preview);
// → written to consolidation_runs.review_artifact_path (Slice 6.9)
```

---

## Anti-Theater

| Do NOT claim | Required proof |
|--------------|----------------|
| "Graph memory shipped" | GRAPH-01 recall@k on frozen queries (Slice 6.5) |
| "Extraction live" | Daemon ingest path calls `run_graph_extract`, not harness-only |
| "Consolidation live" | `consolidation_runs.status=review_pending` until explicit apply |
| **"Phase 6 complete"** | Darwin **15/15 (15 hard)** · Linux **10/15 fail-closed** · README matches harness |

---

## Key Files

| Path | Role |
|------|------|
| `crates/aether-core/src/graph_extract.rs` | Prompt, Ollama call, validation, entity cap |
| `crates/aether-core/schemas/graph_extract.schema.json` | Pinned extraction contract |
| `crates/aether-daemon/src/ingest.rs` | Post-turn hook, audit on failure |
| `crates/aether-daemon/src/task_runner.rs` | Wires ingest after stream/loop `done` |
| `crates/aether-db/src/graph.rs` | CRUD, 1-hop traversal, bi-temporal filters |
| `crates/aether-db/src/consolidate_review.rs` | Consolidate preview diff + markdown review artifact |
| `scripts/consolidate_memory.sh` | Offline consolidate CLI (Slice 6.9) |
| `docs/RATEL_TOOL_INDEX.md` | Ratel BM25 progressive disclosure pattern (SKILL-02) |
| `docs/LINUX_CI.md` | Linux 10/15 fail-closed matrix; Darwin 15/15 gate |
| `docs/ROADMAP_PHASE_6.md` | Phase 6 binding spec · independent audit checklist |

---

*Graph v1 · Phase 6 complete · 2026-07-25*
