---
name: forge-api-guide
description: Progressive-disclosure reference for AetherForge daemon APIs (SKILL-02 fixture)
metadata:
  source: fixtures/book_skill
  routing_tags: [daemon, tcp, ingest, graph]
  chapter_count: 2
---

# Forge API Guide

Multi-chapter skill fixture for SKILL-02 prep. Layout borrowed from
awesome-claude-skills / anthropics progressive-disclosure pattern: frontmatter
metadata + `chapters/` for on-demand routing.

## When to use

- Questions about daemon TCP protocol or session ingest
- Routing to a specific chapter by topic keyword

## Chapters

| Chapter | File | Topics |
|---------|------|--------|
| 1 | `chapters/01-getting-started.md` | Daemon startup, TCP port 7433 |
| 2 | `chapters/02-ingest-pipeline.md` | Post-turn ingest, graph_extract bounds |

## Routing hints

- "TCP" or "port" → chapter 1
- "ingest" or "graph_extract" → chapter 2

## Citation policy (SKILL-02)

Answers must quote verbatim spans from the matched chapter file.
Do not invent API names not present in the source chapter.
