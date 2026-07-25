---
name: book_skill
description: Distilled daemon + graph + recovery reference (skill-creator SKILL-02 fixture)
routing_keywords: daemon-lifecycle, graph-ingest, crash-recovery, ingest-turn
chapters_dir: chapters
source_doc: references/source.md
source_uri: fixtures/skills/book_skill/references/source.md
harness_fixture_id: SKILL-02
---

# Book Skill (skill-creator layout)

Progressive-disclosure skill produced from `references/source.md` per sandiiarov/skill-creator
patterns. Agents load this root file for routing metadata, then fetch individual chapters on demand.

## When to use

- User asks about daemon bind address or startup → `chapters/01-daemon-lifecycle.md`
- User asks about post-turn graph extraction or ingest audit → `chapters/02-graph-ingest.md`
- User asks about WAL recovery or undo journal revert → `chapters/03-crash-recovery.md`

## Chapters

| Chapter | Path | Routing trigger |
|---------|------|-----------------|
| Daemon lifecycle | chapters/01-daemon-lifecycle.md | `aether-daemon`, listen address, startup |
| Graph ingest | chapters/02-graph-ingest.md | `graph_extract`, `ingest_turn`, wiki zone |
| Crash recovery | chapters/03-crash-recovery.md | `RecoveryManager`, undo journal, SIGTERM |

## References

Pinned source for citation fidelity checks: `references/source.md`

## Gotchas

- Default daemon bind is loopback-only (`127.0.0.1:7878`); wide bind needs network grant.
- Ingest failures must audit as `ingest_turn` — never silent skip (Meetily borrow).
- Recovery reverts pending undo rows; raw conversations are immutable.

## Citation policy (SKILL-02)

Answers must include a verbatim span from the cited chapter or `references/source.md`.
Fuzzy match threshold **≥ 0.9** — no hallucinated API names. See `skill_eval_rubric.json`.
