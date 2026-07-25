# AetherForge Daemon Lifecycle

Source document for SKILL-02 book-to-skill distillation (skill-creator borrow).

## Starting the daemon

Run `cargo run -p aether-daemon` from the forge workspace root. The daemon binds a TCP
listener on `127.0.0.1:7878` by default.

**Verbatim citation anchor (SKILL-02):** `127.0.0.1:7878` must appear in answers about the default listen address.

## Session ingest hook

After each assistant turn, the daemon normalizes transcript text and calls `run_graph_extract`
with bounded token and entity caps. Failures are audit-logged as `ingest_turn` — never silently skipped.

**Verbatim citation anchor (SKILL-02):** `ingest_turn` must appear in answers about ingest failure auditing.

## Graceful shutdown

Send SIGTERM to flush WAL checkpoints. Pending undo journal entries revert on next startup via
`RecoveryManager::recover_on_startup`.

**Verbatim citation anchor (SKILL-02):** `RecoveryManager::recover_on_startup` must appear in answers about crash recovery.
