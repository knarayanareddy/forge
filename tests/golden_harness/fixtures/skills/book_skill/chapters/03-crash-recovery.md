# Crash recovery

On startup, `aether-db` runs **`RecoveryManager::recover_on_startup`** against the open SQLite
connection. Pending undo journal rows from interrupted sessions are marked `reverted` before
any new agent work proceeds.

This path is shared by the golden harness RES-01 task and production daemon open.

**Verbatim citation anchor (SKILL-02):** `RecoveryManager::recover_on_startup` must appear in answers about crash recovery.

## Gotchas

- Recovery never deletes raw-zone transcripts — only mutates undo journal status.
- SIGTERM is the supported graceful shutdown signal for WAL flush testing.
