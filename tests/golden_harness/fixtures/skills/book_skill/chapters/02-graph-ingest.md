# Graph ingest pipeline

Post-turn ingest runs asynchronously after stream completion:

1. Normalize whitespace and truncate to `max_tokens_per_batch` (default 4096 estimated tokens).
2. Call Ollama JSON-mode extraction with pinned `graph_extract.schema.json`.
3. Validate via `validate_graph_extract()`; cap entities with `enforce_max_entities()`.
4. Insert nodes/edges into the wiki zone for the active session.

On validation or insert failure the daemon writes an audit entry with tool name **`ingest_turn`**
and decision `denied`.

**Verbatim citation anchor (SKILL-02):** `ingest_turn` must appear in answers about ingest failure auditing.

## Gotchas

- Empty turns still produce audit rows under `LogAndContinue` policy.
- Node ids are session-namespaced to prevent cross-session graph bleed.
