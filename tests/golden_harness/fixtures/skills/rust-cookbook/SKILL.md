---
name: rust-cookbook
description: Progressive-disclosure reference for Rust API patterns (SKILL-02 prep fixture)
routing_keywords: error-handling, async-runtime, testing
chapters_dir: chapters
source_doc: fixtures/book_skill/
---

# Rust Cookbook Skill

Multi-chapter skill layout borrowed from awesome-claude-skills / anthropics progressive-disclosure
patterns. Agents load the root SKILL.md for routing metadata, then pull individual chapter files
on demand — not the full corpus in one prompt.

## When to use

- User asks about `Result` / `?` error propagation → route to `chapters/01-error-handling.md`
- User asks about Tokio / async tasks → route to `chapters/02-async-runtime.md`
- User asks about unit vs integration tests → route to `chapters/03-testing.md`

## Chapters

| Chapter | Path | Routing trigger |
|---------|------|-----------------|
| Error handling | chapters/01-error-handling.md | `Result`, `?`, `thiserror` |
| Async runtime | chapters/02-async-runtime.md | `tokio`, `spawn`, `async fn` |
| Testing | chapters/03-testing.md | `#[test]`, `cargo test`, mocks |

## Citation policy (SKILL-02)

Answers must include a verbatim span from the cited chapter. Fuzzy match threshold ≥ 0.9 against
source fixture text — no hallucinated crate or API names.
