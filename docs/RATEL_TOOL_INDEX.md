# Ratel-Style Progressive Tool & Skill Disclosure

**Phase 6 pre-6.5 borrow** · Pattern only — no `ratel-mcp` runtime  
**Binding spec:** [ROADMAP_PHASE_6.md](./ROADMAP_PHASE_6.md) · Ecosystem v3 research canvas

---

## Thesis

[Ratel](https://github.com/ratel-ai/ratel) is a context-engineering layer: full MCP tool and skill catalogs stay **out of the system prompt**. Each turn, a deterministic BM25 index ranks capabilities by query relevance and injects only the top-k matches (progressive disclosure).

AetherForge already ships a **closed ReAct tool surface** (LOOP-01/02 verify shell). We borrow Ratel's **routing metadata pattern**, not the ratel-mcp gateway or ratel-ai-core dependency.

| Ratel concept | Forge mapping |
|---------------|---------------|
| Separate tool + skill indexes | `aether-mcp::ToolIndex` + `aether-skills::DisclosureIndex` |
| BM25 per-turn search | Lite BM25 in Rust (no vector DB required) |
| Progressive inject top-k | SKILL-02 chapter routing; MCP tool hints at grant time |
| ~80% token savings on overload | Closed surface is already small; pattern prep for 6.8 |

---

## Architecture

```
User query (turn N)
    │
    ├─► ToolIndex::search(query, k)     ──► top-k MCP tool names + descriptions
    │
    └─► DisclosureIndex::search(query, k) ──► top-k skill roots / chapters
              │
              └─► Agent loads matched SKILL.md chapter on demand (not full corpus)
```

**Three-zone alignment:** schema zone (`skills/*/SKILL.md`) is indexed separately from wiki zone (`graph_nodes`). Graph RRF remains the third retrieval signal — BM25 routing does not replace hybrid search.

---

## Implementation (pre-6.5)

| Path | Role |
|------|------|
| `crates/aether-skills/src/disclosure.rs` | BM25-lite rank over skill descriptions + chapter text |
| `crates/aether-mcp/src/tool_index.rs` | BM25-lite rank over `McpToolInfo` descriptions |
| `tests/golden_harness/fixtures/skills/*/SKILL.md` | `routing_keywords` frontmatter for SKILL-02 fixtures |

### Index fields (searchable corpus)

**Skills:** `name`, `description`, `routing_keywords` (frontmatter), chapter filenames + first heading line.

**MCP tools:** `name`, `description` from `tools/list` audit snapshot.

### Query API

```rust
// aether-skills
let index = DisclosureIndex::from_skill_dir(path)?;
let hits = index.search("tokio async runtime", 3);

// aether-mcp
let index = ToolIndex::from_tools(&tools);
let hits = index.search("read file workspace", 2);
```

Both return `(id, score)` pairs sorted by descending BM25 score.

---

## SKILL-02 routing metadata

Progressive-disclosure skills expose frontmatter tags consumed by the disclosure index:

```yaml
---
name: rust-cookbook
description: Progressive-disclosure reference for Rust API patterns
routing_keywords: error-handling, async-runtime, testing
chapters_dir: chapters
source_doc: fixtures/book_skill/
---
```

Slice 6.8 **SKILL-02** harness will assert: given a frozen question, top-1 chapter match + citation span ≥ 0.9 fidelity against `references/source.md`.

---

## Deferrals

| Item | Defer until | Rationale |
|------|-------------|-----------|
| **ratel-mcp server** | LOOP-02 green (Slice 6.6+) | Adds MCP indirection to closed ReAct surface |
| **ratel-ai-core crate** | Tool count > 20 + measured overload | In-process lite BM25 sufficient for Phase 6 |
| **Semantic hybrid in index** | Phase 7+ | FTS5-first hybrid RRF already authoritative |
| **Per-turn auto-inject into prompt** | SKILL-02 hard green | Index + fixtures pinned now; daemon wiring later |

---

## Anti-theater

| Do NOT claim | Required proof |
|--------------|----------------|
| "Ratel integrated" | No ratel-mcp in allowlist or ReAct loop |
| "80% token savings" | No production prompt diff measured pre-6.8 |
| "Skill routing shipped" | SKILL-02 harness recall + citation fidelity (Slice 6.8) |

Unit tests in `aether-skills` and `aether-mcp` prove: given N indexed items, query returns expected top-k ordering on frozen keyword overlap.

---

*Ratel borrow · docs + lite BM25 stubs · 2026-07-25*
