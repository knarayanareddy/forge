# Ingest Pipeline

Session turns flow through the raw zone (`conversations`) before wiki-zone graph insert.

## Batch bounds (Slice 6.4 prep)

- `max_tokens_per_batch`: 4096 estimated tokens per ingest slice
- `max_entities_per_turn`: 32 entities cap for `graph_extract`

Failures are audit-logged under tool name `ingest_turn` — never silently skipped.

## graph_extract schema

Ollama extraction must emit JSON matching `graph_extract.schema.json` with mandatory
`evidence_text` and provenance (`extracted` | `inferred`) on every node and edge.
